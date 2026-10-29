//! Stateless camera commands shared by the navigation cube and programmatic callers.
use serde_json::{Value, json};
use solidcraft_render::{Camera, StandardView};

use super::CommandSpec;
use crate::params::{bad, num, req_vec2, req_vec3, vec3};
use crate::schema::view_navigation::{RenderBrowser, RenderFocus, RenderSelect, TabStep, TabStepParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Sel, Session};

const FOCUS_CONTRACT: TypedContract<RenderFocus> = TypedContract::new();
const BROWSER_CONTRACT: TypedContract<RenderBrowserRequest> = TypedContract::new();
const TAB_STEP_CONTRACT: TypedContract<TabStepParams> = TypedContract::new();

// Both existing request forms share the Browser command and its published schema.
#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
enum RenderBrowserRequest {
    Open(RenderBrowser),
    Controls(RenderBrowserControls),
}

#[derive(serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct RenderBrowserControls {
    #[serde(skip_serializing_if = "Option::is_none")]
    by_look: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
}

const RENDER_SELECT: TypedContract<RenderSelect> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
CommandSpec::new("view.fit", "Fit", fit)
    .icon("fit").key("F6").noundo().doc_only().params("no parameters → frame the active canvas or selection; display only"),
CommandSpec::new("view.home", "Home view", home)
    .icon("home").noundo().doc_only().params("no parameters → home orientation framing the active canvas; display only"),
CommandSpec::new("view.tool", "Navigation tool", tool)
    .icon("orbit").noundo().doc_only().params("tool: orbit|pan|zoom|none → tool; latch a camera tool, none releases it"),
CommandSpec::new("view.spin", "Continuous orbit", spin)
    .icon("spin").noundo().doc_only().params("on?: bool (omitted: toggle) → on?; continuous orbit until the next click or key"),
CommandSpec::new("view.visual_style", "Visual style", visual_style)
    .icon("style").noundo().doc_only().params("style: shaded_with_edges|shaded|wireframe|shaded_with_hidden_edges → style, index (0..3 in that order)"),
CommandSpec::new("view.grid", "Grid", grid)
    .icon("style").noundo().doc_only().params("on?: bool (omitted: toggle) → on?; show the viewport grid"),
CommandSpec::new("view.ground_shadow", "Ground shadow", ground_shadow)
    .icon("style").noundo().doc_only().params("on?: bool (omitted: toggle) → on?; show the ground shadow"),
CommandSpec::new("view.navigation_bar", "Navigation bar", navigation_bar)
    .icon("pan").noundo().doc_only().params("visible?: bool (omitted: toggle) → visible?; show the navigation bar (window setting)"),
CommandSpec::new("view.render_focus", "Render Focus View", crate::checked!("view.render_focus", render_focus))
    .icon("render_scene").noundo().doc_only()
    .contract(&FOCUS_CONTRACT).output(output_schema_for::<RenderFocus>)
    .params("on: bool → {on}; per-window UI state, never saved in the design; headless validation only"),
CommandSpec::new("view.render_frame", "Render Frame Guides", crate::checked!("view.render_frame", render_frame))
    .icon("camera").noundo().doc_only()
    .contract(&RENDER_FRAME_CONTRACT)
    .output(crate::schema::output_schema_for::<crate::schema::view_navigation::RenderFrame>)
    .params("aspect?: 16:9|3:2|1:1|4:5|9:16|custom; thirds?, safe_area?, centre_mark?: bool. At least one field. Validated transient overlay request, echoed headlessly; never saves guides, changes output size, evaluates geometry or opens a dialog"),
CommandSpec::new("view.render_browser", "Render Browser", crate::checked!("view.render_browser", render_browser))
    .icon("list").noundo().doc_only()
    .contract(&BROWSER_CONTRACT).output(output_schema_for::<RenderBrowserRequest>)
    .params("open: bool; or by_look?: bool; search?: bool; query?: string (up to 1024 bytes) → validated transient Browser controls; never edits the design or opens a dialog"),
CommandSpec::new("view.render_inspector", "Render Inspector", render_inspector)
    .icon("inspect").noundo().doc_only()
    .params("visible: bool → {visible}; per-document Inspector visibility; never edits the design or opens a dialog"),
CommandSpec::new("view.render_select", "Render selection", crate::checked!("view.render_select", render_select))
    .contract(&RENDER_SELECT).output(output_schema_for::<RenderSelect>)
    .icon("select").noundo().doc_only()
    .params("kind: nothing|environment|lighting|light|shot|output|wear|sampling|effects|check; id?: bounded nonempty string for light/shot → validated transient Browser focus; never edits the design or opens a dialog"),
CommandSpec::new("view.render_stage", "Render Stage", render_stage)
    .icon("render_scene").noundo().doc_only()
    .params("stage: look|light|frame|output → {stage}; per-document UI state, never edits or evaluates the design; headless validation only"),
CommandSpec::new("view.shots_strip", "Shots Strip", shots_strip)
    .icon("camera").noundo().doc_only()
    .params("visible: bool → {visible}; Render Shots strip UI preference. Headless echo only, no document edits or undo"),
CommandSpec::new("view.render_dock", "Move Render Dock", crate::checked!("view.render_dock", render_dock))
    .at("RENDER", "SCENE").icon("settings").noundo().doc_only()
    .contract(&RENDER_DOCK_CONTRACT)
    .output(crate::schema::output_schema_for::<crate::schema::view_navigation::RenderDock>)
    .params("panel: inspector|browser; side: left|right → {panel, side}; Render dock UI preference; headless echo only, no document edits or undo"),
CommandSpec::new("view.layout", "Workspace layout", layout)
    .icon("settings").noundo().doc_only().params("mode: floating|compact → validated workspace layout; display only, never opens a dialog"),
CommandSpec::new("view.history_layout", "History layout", history_layout)
    .icon("settings").noundo().doc_only().params("width?: 1..1000000; dock?: bottom|left|under_browser|browser (browser: the history in the browser, no timeline); collapsed?: bool → validated layout request; display only, never opens a dialog"),
CommandSpec::new("view.graphics_preset", "Graphics Preset", graphics_preset)
    .icon("settings").noundo().doc_only().params("preset: performance|quality → preset, enabled, display_quality, supersampling, supersampling_adaptive; Quality: AO, Fine, adaptive 2x SSAA; Performance: no AO, Normal, no SSAA"),
CommandSpec::new("view.ambient_occlusion", "Ambient Occlusion", ambient_occlusion)
    .icon("settings").noundo().doc_only().params("enabled: bool → enabled; apply soft contact shading in the GPU viewport"),
CommandSpec::new("view.perspective", "Perspective view", perspective)
    .icon("perspective").noundo().doc_only().params("enabled: bool → enabled; draw the view in perspective, or orthographic when false; a sketch being edited is always shown orthographic"),
CommandSpec::new("view.angle_readout", "View Angles", crate::checked!("view.angle_readout", angle_readout))
    .icon("camera").noundo().doc_only().contract(&ANGLE_READOUT_CONTRACT).output(crate::schema::output_schema_for::<crate::schema::view_navigation::AngleReadoutResult>).params("enabled: bool → enabled; show the view's pitch, yaw and roll (degrees, as view.apply's camera in radians) and Ortho or Persp under the orientation gizmo; display only, never opens a dialog"),
CommandSpec::new("view.navigate", "Navigate View", navigate)
    .icon("orbit")
    .noundo()
    .doc_only()
    .params("camera: Camera; direction?: [x,y,z] in the camera's up-axis frame; or action: left|right|up|down|clockwise|counterclockwise|home|fit, degrees?: 0..180 (how far left|right|up|down turn; default 90); or motion: pan|orbit|zoom, delta?: [dx,dy] (pixels, each ±10000), free_orbit?: bool (orbit through poles, default false), viewport?: [width,height] (1..1000000, required for pan or pivot), pivot?: [x,y,z], factor?: 0.05..20 (required for zoom), anchor?: [x,y,z] → camera, fit, continuous?"),
CommandSpec::new("view.normal_to", "Normal to Selection", normal_to)
        .icon("camera")
        .noundo()
        .enabled(|s| normal_target(s).map(|_| ()).ok_or_else(|| "select a planar face or plane".into()))
        .params("camera?: Camera (default: current session camera or home view) → camera; Look At the first selected planar face or plane, centered on the picked point; repeat while aligned to flip the side; preserves scale and projection"),
CommandSpec::new("view.front", "Front View", front).icon("camera").noundo().key("Ctrl+1").params(STANDARD_PARAMS),
CommandSpec::new("view.back", "Back View", back).icon("camera").noundo().key("Ctrl+2").params(STANDARD_PARAMS),
CommandSpec::new("view.left", "Left View", left).icon("camera").noundo().key("Ctrl+3").params(STANDARD_PARAMS),
CommandSpec::new("view.right", "Right View", right).icon("camera").noundo().key("Ctrl+4").params(STANDARD_PARAMS),
CommandSpec::new("view.top", "Top View", top).icon("camera").noundo().key("Ctrl+5").params(STANDARD_PARAMS),
CommandSpec::new("view.bottom", "Bottom View", bottom).icon("camera").noundo().key("Ctrl+6").params(STANDARD_PARAMS),
CommandSpec::new("view.isometric", "Isometric View", isometric).icon("camera").noundo().key("Ctrl+7").params(STANDARD_PARAMS),
CommandSpec::new("view.next_tab", "Next Tab", crate::checked!("view.next_tab", next_tab))
    .icon("chevron_right").noundo().doc_only().key("Ctrl+Tab").contract(&TAB_STEP_CONTRACT).output(output_schema_for::<TabStep>)
    .params("no parameters → step: 1; the window shows the design tab to the right of the shown one (after the last, the first); never edits the design or opens a dialog"),
CommandSpec::new("view.previous_tab", "Previous Tab", crate::checked!("view.previous_tab", previous_tab))
    .icon("chevron_right").noundo().doc_only().key("Ctrl+Shift+Tab").contract(&TAB_STEP_CONTRACT).output(output_schema_for::<TabStep>)
    .params("no parameters → step: -1; the window shows the design tab to the left of the shown one (before the first, the last); never edits the design or opens a dialog")];

const STANDARD_PARAMS: &str = "camera?: Camera (default: current session camera or home view) → camera; turns to that standard view as the view cube does, keeping target, distance and projection";

/// The standard view a `view.front` … `view.isometric` command turns to.
pub fn standard_view(id: &str) -> Option<StandardView> {
    Some(match id {
        "view.front" => StandardView::Front,
        "view.back" => StandardView::Back,
        "view.left" => StandardView::Left,
        "view.right" => StandardView::Right,
        "view.top" => StandardView::Top,
        "view.bottom" => StandardView::Bottom,
        "view.isometric" => StandardView::Iso,
        _ => return None,
    })
}

fn front(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.front")
}
fn back(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.back")
}
fn left(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.left")
}
fn right(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.right")
}
fn top(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.top")
}
fn bottom(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.bottom")
}
fn isometric(s: &mut Session, p: &Value) -> Result<Value> {
    turn_to(s, p, "view.isometric")
}

fn turn_to(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let view = standard_view(cmd).ok_or_else(|| bad(cmd, "not a standard view"))?;
    let mut cam: Camera = match p.get("camera") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| bad(cmd, "invalid camera"))?,
        None => s.camera.unwrap_or_else(|| crate::view::home_camera(s)),
    };
    if !cam.is_valid() || !cam.fov.is_finite() || !(0.0..std::f64::consts::PI).contains(&cam.fov) {
        return Err(bad(cmd, "invalid camera"));
    }
    cam.set_view(view);
    s.camera = Some(cam);
    Ok(json!({"camera": cam}))
}

fn normal_target(s: &Session) -> Option<(solidcraft_geom::Vec3, solidcraft_geom::Vec3)> {
    let state = s.world_state();
    s.selection.iter().find_map(|selection| match selection {
        Sel::Face { body, index, point } if point.is_finite() => {
            let plane = state.body(body)?.body.planar_face(*index)?;
            Some((*point, plane.normal()))
        }
        Sel::Plane { name } => solidcraft_geom::Plane::named(name).map(|plane| (plane.origin, plane.normal())),
        _ => None,
    })
}

fn normal_to(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.normal_to";
    let (point, normal) = normal_target(s).ok_or_else(|| bad(cmd, "select a planar face or plane"))?;
    let cam: Camera = match p.get("camera") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| bad(cmd, "invalid camera"))?,
        None => s.camera.unwrap_or_else(|| crate::view::home_camera(s)),
    };
    if !cam.is_valid()
        || !cam.fov.is_finite()
        || !(0.0..std::f64::consts::PI).contains(&cam.fov)
        || cam.yaw.abs() > 1e6
        || cam.roll.abs() > 1e6
        || cam.pitch.abs() > std::f64::consts::FRAC_PI_2
    {
        return Err(bad(cmd, "invalid camera"));
    }
    // Promoted from viewport::look_at_selection: asking again from the normal side flips it.
    let direction = if cam.back().dot(normal) > 0.999 { -normal } else { normal };
    let mut to = cam.looking_from(direction);
    to.target = point;
    s.camera = Some(to);
    Ok(json!({"camera": to}))
}

// UI-owned settings follow the layout command: validate here, apply in the UI adapter.
fn view_params(p: &Value, cmd: &str, allowed: &[&str]) -> Result<()> {
    let object = p.as_object().ok_or_else(|| bad(cmd, "parameters must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(bad(cmd, format!("unknown parameter: {key}")));
    }
    Ok(())
}

fn fit(_s: &mut Session, p: &Value) -> Result<Value> {
    view_params(p, "view.fit", &[])?;
    Ok(json!({}))
}

fn home(_s: &mut Session, p: &Value) -> Result<Value> {
    view_params(p, "view.home", &[])?;
    Ok(json!({}))
}

fn tool(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.tool";
    view_params(p, cmd, &["tool"])?;
    let tool = p
        .get("tool")
        .and_then(Value::as_str)
        .filter(|v| matches!(*v, "orbit" | "pan" | "zoom" | "none"))
        .ok_or_else(|| bad(cmd, "tool must be orbit, pan, zoom or none"))?;
    Ok(json!({"tool": tool}))
}

fn visual_style(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.visual_style";
    view_params(p, cmd, &["style"])?;
    let style = p.get("style").and_then(Value::as_str);
    let index = match style {
        Some("shaded_with_edges") => 0,
        Some("shaded") => 1,
        Some("wireframe") => 2,
        Some("shaded_with_hidden_edges") => 3,
        _ => return Err(bad(cmd, "style must be shaded_with_edges, shaded, wireframe or shaded_with_hidden_edges")),
    };
    Ok(json!({"style": style, "index": index}))
}

fn view_toggle(p: &Value, cmd: &str, key: &str) -> Result<Value> {
    view_params(p, cmd, &[key])?;
    match p.get(key) {
        Some(value) => value.as_bool().map(|on| json!({key: on})).ok_or_else(|| bad(cmd, format!("{key} must be a boolean"))),
        None => Ok(json!({})),
    }
}

fn spin(_s: &mut Session, p: &Value) -> Result<Value> {
    view_toggle(p, "view.spin", "on")
}
fn grid(_s: &mut Session, p: &Value) -> Result<Value> {
    view_toggle(p, "view.grid", "on")
}
fn ground_shadow(_s: &mut Session, p: &Value) -> Result<Value> {
    view_toggle(p, "view.ground_shadow", "on")
}
fn navigation_bar(_s: &mut Session, p: &Value) -> Result<Value> {
    view_toggle(p, "view.navigation_bar", "visible")
}

fn render_focus(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: RenderFocus = serde_json::from_value(p.clone()).map_err(|e| bad("view.render_focus", e.to_string()))?;
    Ok(json!(request))
}

fn render_select(s: &mut Session, p: &Value) -> Result<Value> {
    let request: RenderSelect = serde_json::from_value(p.clone()).map_err(|e| bad("view.render_select", e.to_string()))?;
    let result = serde_json::to_value(request).map_err(|e| bad("view.render_select", e.to_string()))?;
    if !s.selection.is_empty() {
        s.selection.clear();
        s.revision += 1;
    }
    Ok(result)
}

fn render_browser(_s: &mut Session, p: &Value) -> Result<Value> {
    if !p.is_object() {
        return Err(bad("view.render_browser", "expected an object with Browser controls"));
    }
    if p.get("open").is_some() {
        let request: RenderBrowser = serde_json::from_value(p.clone()).map_err(|e| bad("view.render_browser", e.to_string()))?;
        return Ok(json!(request));
    }
    let request: RenderBrowserControls = serde_json::from_value(p.clone()).map_err(|e| bad("view.render_browser", e.to_string()))?;
    if request.by_look.is_none() && request.search.is_none() && request.query.is_none() {
        return Err(bad("view.render_browser", "expected at least one Browser control"));
    }
    if request.query.as_ref().is_some_and(|q| q.len() > 1024) {
        return Err(bad("query", "expected at most 1024 bytes"));
    }
    serde_json::to_value(request).map_err(|e| bad("view.render_browser", e.to_string()))
}

static RENDER_FRAME_CONTRACT: crate::schema::TypedContract<crate::schema::view_navigation::RenderFrame> = crate::schema::TypedContract::new();

fn render_frame(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.render_frame";
    let request: crate::schema::view_navigation::RenderFrame = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if request.aspect.is_none() && request.thirds.is_none() && request.safe_area.is_none() && request.centre_mark.is_none() {
        return Err(bad(cmd, "expected at least one frame control"));
    }
    serde_json::to_value(request).map_err(|e| bad(cmd, e.to_string()))
}

fn render_stage(_s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        stage: String,
    }
    let request: Request = serde_json::from_value(p.clone()).map_err(|e| bad("view.render_stage", e.to_string()))?;
    if !matches!(request.stage.as_str(), "look" | "light" | "frame" | "output") {
        return Err(bad("stage", "expected look, light, frame or output"));
    }
    Ok(json!({"stage":request.stage}))
}

const RENDER_DOCK_CONTRACT: crate::schema::TypedContract<crate::schema::view_navigation::RenderDock> = crate::schema::TypedContract::new();

fn render_dock(_s: &mut Session, p: &Value) -> Result<Value> {
    let request: crate::schema::view_navigation::RenderDock =
        serde_json::from_value(p.clone()).map_err(|e| bad("view.render_dock", e.to_string()))?;
    serde_json::to_value(request).map_err(|e| bad("view.render_dock", e.to_string()))
}

fn render_inspector(_s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        visible: bool,
    }
    let request: Request = serde_json::from_value(p.clone()).map_err(|e| bad("view.render_inspector", e.to_string()))?;
    Ok(json!({"visible":request.visible}))
}

fn shots_strip(_s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        visible: bool,
    }
    let request: Request = serde_json::from_value(p.clone()).map_err(|e| bad("view.shots_strip", e.to_string()))?;
    Ok(json!({"visible": request.visible}))
}

fn layout(_s: &mut Session, p: &Value) -> Result<Value> {
    let mode = p
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "floating" | "compact"))
        .ok_or_else(|| bad("view.layout", "mode must be floating or compact"))?;
    Ok(json!({"mode": mode}))
}

fn history_layout(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.history_layout";
    let mut result = serde_json::Map::new();
    if let Some(value) = p.get("width") {
        let width = value
            .as_f64()
            .filter(|v| v.is_finite() && (1.0..=1_000_000.0).contains(v))
            .ok_or_else(|| bad(cmd, "width must be finite and within 1..1000000"))?;
        result.insert("width".into(), json!(width));
    }
    if let Some(value) = p.get("dock") {
        let dock = value
            .as_str()
            .filter(|v| matches!(*v, "bottom" | "left" | "under_browser" | "browser"))
            .ok_or_else(|| bad(cmd, "dock must be bottom, left, under_browser or browser"))?;
        result.insert("dock".into(), json!(dock));
    }
    if let Some(value) = p.get("collapsed") {
        let collapsed = value.as_bool().ok_or_else(|| bad(cmd, "collapsed must be a boolean"))?;
        result.insert("collapsed".into(), json!(collapsed));
    }
    Ok(Value::Object(result))
}

fn navigate(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.navigate";
    let mut cam: Camera =
        serde_json::from_value(p.get("camera").cloned().ok_or_else(|| bad(cmd, "camera is required"))?).map_err(|_| bad(cmd, "invalid camera"))?;
    if !cam.is_valid()
        || !cam.fov.is_finite()
        || !(0.0..std::f64::consts::PI).contains(&cam.fov)
        || cam.yaw.abs() > 1e6
        || cam.roll.abs() > 1e6
        || cam.pitch.abs() > std::f64::consts::FRAC_PI_2
    {
        return Err(bad(cmd, "invalid camera"));
    }
    if p.get("motion").is_some() {
        return continuous(cam, p);
    }
    let mut fit = false;
    if let Some(value) = p.get("direction") {
        let dir = vec3(value)
            .filter(|v| v.is_finite() && v.len() <= 2.0)
            .and_then(|v| v.normalized())
            .ok_or_else(|| bad(cmd, "direction must be a finite nonzero vector of length at most 2"))?;
        let heading = cam.yaw;
        cam = cam.looking_from(cam.from_frame(dir));
        if dir.x.abs() < 1e-9 && dir.y.abs() < 1e-9 {
            cam.yaw = square_heading(heading);
        }
    } else {
        use std::f64::consts::FRAC_PI_2;
        let (r, u, b) = cam.basis();
        // Left, right, up and down turn the view a quarter turn (the navigation cube's arrows),
        // or `degrees` (the arrow keys' 15° steps, #1541).
        let (sin, cos) = match p.get("degrees") {
            None => (1.0, 0.0),
            Some(d) => d
                .as_f64()
                .filter(|d| d.is_finite() && *d > 0.0 && *d <= 180.0)
                .map(|d| if d == 90.0 { (1.0, 0.0) } else { d.to_radians().sin_cos() })
                .ok_or_else(|| bad(cmd, "degrees must be a number above 0 and at most 180"))?,
        };
        let turn = |toward: solidcraft_geom::Vec3| cam.looking_from(b * cos + toward * sin);
        match p.get("action").and_then(Value::as_str) {
            Some("left") => cam = turn(-r),
            Some("right") => cam = turn(r),
            Some("up") => cam = turn(u),
            Some("down") => cam = turn(-u),
            Some("clockwise") => cam.roll = (cam.roll + FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            Some("counterclockwise") => cam.roll = (cam.roll - FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            Some("home") => {
                cam.set_view(StandardView::Iso);
                fit = true;
            }
            Some("fit") => fit = true,
            _ => return Err(bad(cmd, "unknown navigation action")),
        }
    }
    Ok(json!({"camera": cam, "fit": fit}))
}

/// Direct manipulation is stateless, bounded and independent of document evaluation.
fn continuous(mut cam: Camera, p: &Value) -> Result<Value> {
    let cmd = "view.navigate";
    let free_orbit = match p.get("free_orbit") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| bad(cmd, "free_orbit must be a boolean"))?,
    };
    let viewport = || {
        let size = req_vec2(cmd, p, "viewport")?;
        if !(1.0..=1e6).contains(&size.x) || !(1.0..=1e6).contains(&size.y) {
            return Err(bad(cmd, "viewport dimensions must be in 1..1000000"));
        }
        Ok(size)
    };
    match p.get("motion").and_then(Value::as_str) {
        Some(motion @ ("pan" | "orbit")) => {
            let delta = req_vec2(cmd, p, "delta")?;
            if delta.x.abs() > 1e4 || delta.y.abs() > 1e4 {
                return Err(bad(cmd, "motion deltas must be within ±10000 pixels"));
            }
            let orbit = |cam: &mut Camera| {
                if free_orbit {
                    cam.rotate_in_view(solidcraft_geom::Vec3::new(-delta.y * 0.008, -delta.x * 0.008, 0.0));
                } else {
                    cam.orbit(delta.x, delta.y);
                }
            };
            if motion == "pan" {
                cam.pan(delta.x, delta.y, viewport()?.y);
            } else if p.get("pivot").is_some() {
                let pivot = req_vec3(cmd, p, "pivot")?;
                viewport()?;
                let coords = cam.frame_coords(pivot);
                orbit(&mut cam);
                cam.place(pivot, coords);
            } else {
                orbit(&mut cam);
            }
        }
        Some("zoom") => {
            let factor = num(p, "factor").filter(|f| (0.05..=20.0).contains(f)).ok_or_else(|| bad(cmd, "factor must be in 0.05..20"))?;
            let anchor = p.get("anchor").map(|_| req_vec3(cmd, p, "anchor")).transpose()?;
            cam.zoom_at(factor, anchor);
        }
        _ => return Err(bad(cmd, "unknown navigation motion")),
    }
    if !cam.is_valid() {
        return Err(bad(cmd, "motion produced an invalid camera"));
    }
    Ok(json!({"camera":cam, "fit":false, "continuous":true}))
}

/// At a halfway tie prefer the heading nearer the front plan view.
pub(crate) fn square_heading(yaw: f64) -> f64 {
    if !yaw.is_finite() {
        return 0.0;
    }
    let q = std::f64::consts::FRAC_PI_2;
    let lo = (yaw / q).floor() * q;
    let hi = lo + q;
    if ((yaw - lo) - (hi - yaw)).abs() < 1e-6 {
        let off_front = |a: f64| ((a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs();
        if off_front(lo) <= off_front(hi) { lo } else { hi }
    } else if yaw - lo < hi - yaw {
        lo
    } else {
        hi
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    use solidcraft_render::UpAxis;

    #[test]
    fn view_bar_commands_validate_json_without_editing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for id in ["view.fit", "view.home"] {
            assert_eq!(s.execute(id, &json!({})).unwrap(), json!({}));
            assert_eq!(s.execute(id, &Value::Null).unwrap(), json!({}));
            assert!(s.execute(id, &json!({"unexpected": true})).is_err());
        }
        for tool in ["orbit", "pan", "zoom", "none"] {
            assert_eq!(s.execute("view.tool", &json!({"tool":tool})).unwrap(), json!({"tool":tool}));
        }
        for (index, style) in ["shaded_with_edges", "shaded", "wireframe", "shaded_with_hidden_edges"].into_iter().enumerate() {
            assert_eq!(s.execute("view.visual_style", &json!({"style":style})).unwrap(), json!({"style":style,"index":index}));
        }
        for (id, key) in [("view.tool", "tool"), ("view.visual_style", "style")] {
            assert!(s.execute(id, &json!({})).is_err());
            assert!(s.execute(id, &Value::Null).is_err());
            for value in [json!("unknown"), json!(""), json!(0), json!(true), Value::Null, json!([]), json!({})] {
                assert!(s.execute(id, &json!({key:value})).is_err(), "{id}: {value}");
            }
        }
        for (id, key) in [("view.spin", "on"), ("view.grid", "on"), ("view.ground_shadow", "on"), ("view.navigation_bar", "visible")] {
            assert_eq!(s.execute(id, &json!({})).unwrap(), json!({}));
            assert_eq!(s.execute(id, &Value::Null).unwrap(), json!({}));
            for on in [false, true] {
                assert_eq!(s.execute(id, &json!({key:on})).unwrap(), json!({key:on}));
            }
            for value in [Value::Null, json!(0), json!("true"), json!([]), json!({})] {
                assert!(s.execute(id, &json!({key:value})).is_err(), "{id}: {value}");
            }
        }
        for id in ["view.fit", "view.home", "view.tool", "view.spin", "view.visual_style", "view.grid", "view.ground_shadow", "view.navigation_bar"] {
            for params in [json!(true), json!(1), json!("orbit"), json!([]), json!({"unexpected":true})] {
                assert!(s.execute(id, &params).is_err(), "{id}: {params}");
            }
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
        assert!(s.camera.is_none());
    }

    #[test]
    fn look_at_planes_uses_the_first_supported_selection_and_flips_on_repeat() {
        let original = Camera { yaw: 0.3, pitch: 0.4, roll: 0.7, fov: 0.6, distance: 123.0, ..Default::default() };
        let mut s = Session {
            selection: vec![Sel::Plane { name: "missing".into() }, Sel::Plane { name: "XY".into() }, Sel::Plane { name: "YZ".into() }],
            camera: Some(original),
            ..Default::default()
        };
        for normal in [Vec3::Z, -Vec3::Z, Vec3::Z] {
            let result = s.execute("view.normal_to", &json!({})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!(to.back().dist(normal) < 2e-6);
            assert_eq!((to.target, to.distance, to.fov), (Vec3::ZERO, original.distance, original.fov));
        }
        assert!(s.undo.is_empty());
    }

    #[test]
    fn normal_to_selection_faces_each_planar_surface_without_editing_the_design() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.01).unwrap();
        let document = s.doc.to_json();
        let undo = s.undo.len();
        for up in [UpAxis::Z, UpAxis::Y] {
            for fov in [0.0, 0.6] {
                let cam = Camera { up, fov, roll: 1.2, distance: 123.0, ..Default::default() };
                for face in &faces {
                    s.selection = vec![Sel::Face { body: body.name.clone(), index: face.index, point: face.centroid }];
                    let selected = s.selection.clone();
                    let result = s.execute("view.normal_to", &json!({"camera":cam})).unwrap();
                    let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
                    assert!(to.back().dist(face.plane_normal.unwrap()) < 2e-6);
                    assert_eq!(to.target, face.centroid);
                    assert_eq!((to.distance, to.fov, to.up, to.roll), (cam.distance, cam.fov, cam.up, 0.0));
                    assert_eq!(s.camera, Some(to));
                    assert_eq!(s.selection, selected);
                }
            }
        }
        assert_eq!(s.doc.to_json(), document);
        assert_eq!(s.undo.len(), undo);
    }

    #[test]
    fn standard_view_commands_turn_the_camera_and_keep_the_rest() {
        let cam = Camera { yaw: 0.3, pitch: 0.4, fov: 0.6, distance: 123.0, ..Default::default() };
        let mut s = Session { camera: Some(cam), ..Default::default() };
        for (id, key, view) in [
            ("view.front", "Ctrl+1", StandardView::Front),
            ("view.back", "Ctrl+2", StandardView::Back),
            ("view.left", "Ctrl+3", StandardView::Left),
            ("view.right", "Ctrl+4", StandardView::Right),
            ("view.top", "Ctrl+5", StandardView::Top),
            ("view.bottom", "Ctrl+6", StandardView::Bottom),
            ("view.isometric", "Ctrl+7", StandardView::Iso),
        ] {
            assert_eq!(crate::find_command(id).and_then(|c| c.shortcut), Some(key), "{id}");
            let result = s.execute(id, &json!({"camera": cam})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            let mut want = cam;
            want.set_view(view);
            assert!(to.back().dist(want.back()) < 1e-9, "{id}");
            assert_eq!((to.target, to.distance, to.fov), (cam.target, cam.distance, cam.fov), "{id}");
            assert_eq!(s.camera, Some(to));
        }
        assert!(s.undo.is_empty());
        assert!(s.execute("view.top", &json!({"camera": Camera { distance: -1.0, ..Default::default() }})).is_err());
    }

    #[test]
    fn normal_to_selection_rejects_curved_missing_and_invalid_inputs() {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"diameter":20,"height":30})).unwrap();
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.1).unwrap();
        let flat = faces.iter().find(|f| f.plane_normal.is_some()).unwrap();
        let curved = faces.iter().find(|f| f.plane_normal.is_none()).unwrap();
        let selected = |index, point| Sel::Face { body: body.name.clone(), index, point };
        for selection in [vec![], vec![selected(curved.index, curved.centroid)], vec![selected(usize::MAX, Vec3::ZERO)]] {
            s.selection = selection;
            assert!(s.execute("view.normal_to", &json!({})).is_err());
            assert!(s.camera.is_none());
        }
        s.selection = vec![selected(flat.index, flat.centroid)];
        for camera in [
            json!({}),
            json!({"distance":0}),
            json!(Camera { distance: -1.0, ..Default::default() }),
            json!(Camera { fov: 4.0, ..Default::default() }),
        ] {
            assert!(s.execute("view.normal_to", &json!({"camera":camera})).is_err());
            assert!(s.camera.is_none());
        }
        // Programmatic callers can use the current session view without supplying a camera.
        assert!(s.execute("view.normal_to", &json!({})).is_ok());
    }

    #[test]
    fn normal_to_selection_uses_the_world_normal_of_a_moved_component() {
        let mut s = Session::default();
        s.execute("component.create", &json!({"name":"Part","grounded":false})).unwrap();
        s.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence":"Part:1","translate":[50,20,10],"axis":[1,2,3],"angle":"37 deg"})).unwrap();
        let state = s.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.01).unwrap();
        let cam = Camera { roll: 1.4, ..Default::default() };
        for face in faces {
            s.selection = vec![Sel::Face { body: body.name.clone(), index: face.index, point: face.centroid }];
            let result = s.execute("view.normal_to", &json!({"camera":cam})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!(to.back().dist(face.plane_normal.unwrap()) < 2e-6);
            assert!(to.target.dist(face.centroid) < 1e-9);
        }
    }

    /// #1541: left, right, up and down turn a quarter turn, or `degrees` (the arrow keys' 15°).
    #[test]
    fn turns_take_degrees_and_default_to_a_quarter_turn() {
        let mut s = Session::default();
        let cam = Camera { yaw: 0.0, pitch: 0.0, distance: 100.0, ..Default::default() };
        let mut turn = |action: &str, degrees: Option<Value>| -> Result<Camera> {
            let mut params = json!({"camera": cam, "action": action});
            if let Some(d) = degrees {
                params["degrees"] = d;
            }
            let result = s.execute("view.navigate", &params)?;
            Ok(serde_json::from_value(result["camera"].clone()).unwrap())
        };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // A quarter turn as before, given or not.
        let quarter = turn("left", None).unwrap();
        assert_eq!(turn("left", Some(json!(90))).unwrap(), quarter);
        assert!(close((quarter.yaw - cam.yaw).abs(), std::f64::consts::FRAC_PI_2));
        // 15° each way, about the up axis or over the top.
        for (action, yaw, pitch) in [("left", -15.0, 0.0), ("right", 15.0, 0.0), ("up", 0.0, 15.0), ("down", 0.0, -15.0)] {
            let to = turn(action, Some(json!(15))).unwrap();
            assert!(close(to.yaw.abs(), f64::to_radians(yaw).abs()), "{action}: yaw {}", to.yaw);
            assert!(close(to.pitch, f64::to_radians(pitch)), "{action}: pitch {}", to.pitch);
            assert_eq!((to.target, to.distance), (cam.target, cam.distance));
        }
        // Left and right turn opposite ways.
        assert!(close(turn("left", Some(json!(15))).unwrap().yaw, -turn("right", Some(json!(15))).unwrap().yaw));
        for bad in [json!(0), json!(-15), json!(181), json!(f64::NAN.to_string()), json!("15"), json!(null)] {
            assert!(turn("left", Some(bad.clone())).is_err(), "{bad}");
        }
    }

    #[test]
    fn continuous_navigation_moves_the_camera_without_editing_the_design() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        let cam = Camera { distance: 100.0, ..Default::default() };
        for (motion, extra) in
            [("pan", json!({"delta":[30,20],"viewport":[800,600]})), ("orbit", json!({"delta":[30,20]})), ("zoom", json!({"factor":0.5}))]
        {
            let mut params = extra;
            params["camera"] = json!(cam);
            params["motion"] = json!(motion);
            let result = s.execute("view.navigate", &params).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert_eq!(result["continuous"], true);
            match motion {
                "pan" => {
                    assert_ne!(to.target, cam.target);
                    assert_eq!((to.yaw, to.pitch, to.distance), (cam.yaw, cam.pitch, cam.distance));
                }
                "orbit" => {
                    assert!((to.yaw - cam.yaw - 0.24).abs() < 1e-9);
                    assert!((to.pitch - cam.pitch - 0.16).abs() < 1e-9);
                    assert_eq!((to.target, to.distance), (cam.target, cam.distance));
                }
                _ => assert_eq!(to.distance, 50.0),
            }
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }

    #[test]
    fn continuous_free_orbit_is_opt_in_and_preserves_framing_and_document() {
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut s = Session::default();
            let design = s.doc.to_json();
            let cam = Camera { pitch: 1.4, roll: 0.7, up, ..Default::default() };
            let mut p = json!({"camera":cam,"motion":"orbit","delta":[0,80]});
            let default = s.execute("view.navigate", &p).unwrap();
            p["free_orbit"] = json!(false);
            assert_eq!(s.execute("view.navigate", &p).unwrap(), default);
            let clamped: Camera = serde_json::from_value(default["camera"].clone()).unwrap();
            assert!(clamped.pitch > cam.pitch && clamped.pitch < std::f64::consts::FRAC_PI_2);
            p["free_orbit"] = json!(true);
            let result = s.execute("view.navigate", &p).unwrap();
            let free: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!((free.back().dot(cam.back()) - 0.64_f64.cos()).abs() < 1e-8);
            assert_eq!((free.target, free.distance, free.fov), (cam.target, cam.distance, cam.fov));
            assert_eq!(s.doc.to_json(), design);
            assert!(s.undo.is_empty() && s.selection.is_empty());
        }
    }

    #[test]
    fn continuous_free_orbit_keeps_the_pivot_in_the_camera_frame() {
        for up in [UpAxis::Y, UpAxis::Z] {
            for fov in [0.0, 0.6] {
                let mut s = Session::default();
                let cam = Camera { target: Vec3::new(2.0, 3.0, 4.0), pitch: 1.4, roll: 0.7, fov, up, ..Default::default() };
                for pivot in [Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)] {
                    let result = s
                        .execute(
                            "view.navigate",
                            &json!({"camera":cam,"motion":"orbit","delta":[30,80],"free_orbit":true,"viewport":[800,600],"pivot":pivot}),
                        )
                        .unwrap();
                    let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
                    assert!(to.frame_coords(pivot).dist(cam.frame_coords(pivot)) < 1e-8);
                    assert!(((to.eye() - pivot).len() - (cam.eye() - pivot).len()).abs() < 1e-8);
                    let (ax, ay, _) = cam.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
                    let (bx, by, _) = to.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
                    assert!((ax - bx).abs() < 1e-6 && (ay - by).abs() < 1e-6);
                    assert_eq!((to.distance, to.fov), (cam.distance, cam.fov));
                }
            }
        }
    }

    #[test]
    fn continuous_perspective_orbit_keeps_near_and_far_pivots_on_screen() {
        let mut s = Session::default();
        let cam = Camera { yaw: 0.0, pitch: 0.0, distance: 100.0, fov: 0.6, ..Default::default() };
        for pivot in [Vec3::new(15.0, -50.0, 0.0), Vec3::new(15.0, 50.0, 0.0)] {
            let result =
                s.execute("view.navigate", &json!({"camera":cam,"motion":"orbit","delta":[20,10],"viewport":[800,600],"pivot":pivot})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            let (ax, ay, _) = cam.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
            let (bx, by, _) = to.to_screen(pivot, 800.0, 600.0, 200.0).unwrap();
            assert!((ax - bx).abs() < 1e-6 && (ay - by).abs() < 1e-6, "pivot {pivot:?} drifted from ({ax},{ay}) to ({bx},{by})");
        }
    }

    #[test]
    fn continuous_orbit_keeps_its_pivot_on_screen_and_rejects_hostile_motion() {
        let mut s = Session::default();
        let cam = Camera { distance: 100.0, ..Default::default() };
        let pivot = Vec3::new(12.0, 5.0, 0.0);
        let result = s.execute("view.navigate", &json!({"camera":cam,"motion":"orbit","delta":[20,10],"viewport":[800,600],"pivot":pivot})).unwrap();
        let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
        let (ax, ay, _) = cam.to_screen(pivot, 800.0, 600.0, 1.0).unwrap();
        let (bx, by, _) = to.to_screen(pivot, 800.0, 600.0, 1.0).unwrap();
        assert!((ax - bx).abs() < 1e-6 && (ay - by).abs() < 1e-6);
        for extra in [
            json!({"motion":"pan","delta":[1,2],"viewport":[800,0]}),
            json!({"motion":"pan","delta":[1e100,2],"viewport":[800,600]}),
            json!({"motion":"orbit","delta":[1]}),
            json!({"motion":"orbit","delta":[1,2],"pivot":[0,0,0]}),
            json!({"motion":"orbit","delta":[1,2],"free_orbit":"true"}),
            json!({"motion":"orbit","delta":[1,2],"free_orbit":1}),
            json!({"motion":"orbit","delta":[1,2],"free_orbit":null}),
            json!({"motion":"zoom","factor":0}),
            json!({"motion":"zoom","factor":1e100}),
            json!({"motion":"zoom","factor":1,"anchor":[1e100,0,0]}),
            json!({"motion":"unknown"}),
        ] {
            let mut p = extra;
            p["camera"] = json!(cam);
            assert!(s.execute("view.navigate", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn every_cube_direction_preserves_framing_without_editing_the_design() {
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut s = Session::default();
            let before = s.doc.to_json();
            let cam = Camera { up, target: Vec3::new(10.0, 20.0, 30.0), distance: 123.0, ..Default::default() };
            for x in [-1.0, 0.0, 1.0] {
                for y in [-1.0, 0.0, 1.0] {
                    for z in [-1.0, 0.0, 1.0] {
                        let Some(dir) = Vec3::new(x, y, z).normalized() else { continue };
                        let result = s.execute("view.navigate", &json!({"camera":cam,"direction":[x,y,z]})).unwrap();
                        let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
                        assert!(to.back().dist(cam.from_frame(dir)) < 2e-6);
                        assert_eq!((to.target, to.distance, to.fov, to.up), (cam.target, cam.distance, cam.fov, cam.up));
                    }
                }
            }
            assert_eq!(s.doc.to_json(), before);
            assert!(s.undo.is_empty());
        }
    }

    #[test]
    fn arrows_roll_and_home_are_programmatic_and_reject_hostile_input() {
        let mut s = Session::default();
        let cam = Camera::default();
        let (r, u, _) = cam.basis();
        for (action, dir) in [("left", -r), ("right", r), ("up", u), ("down", -u)] {
            let result = navigate(&mut s, &json!({"camera":cam,"action":action})).unwrap();
            let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
            assert!(to.back().dist(dir) < 2e-6);
        }
        let result = navigate(&mut s, &json!({"camera":cam,"action":"clockwise"})).unwrap();
        let to: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
        assert_eq!(to.back(), cam.back());
        assert!((to.basis().0 - u).len() < 1e-9);
        for action in ["home", "fit"] {
            assert_eq!(navigate(&mut s, &json!({"camera":cam,"action":action})).unwrap()["fit"], true);
        }
        for params in [
            json!({}),
            json!({"camera":cam,"direction":[0,0,0]}),
            json!({"camera":cam,"direction":[1e308,0,0]}),
            json!({"camera":cam,"action":"unknown"}),
        ] {
            assert!(s.execute("view.navigate", &params).is_err());
        }
    }
}

fn ambient_occlusion(_s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("view.ambient_occlusion", "enabled must be a boolean"))?;
    Ok(json!({"enabled": enabled}))
}

fn perspective(_s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("view.perspective", "enabled must be a boolean"))?;
    Ok(json!({"enabled": enabled}))
}

const ANGLE_READOUT_CONTRACT: crate::schema::TypedContract<crate::schema::view_navigation::AngleReadoutParams> = crate::schema::TypedContract::new();

/// The View angles readout under the orientation gizmo (#6032): a display setting the UI keeps.
fn angle_readout(_s: &mut Session, p: &Value) -> Result<Value> {
    let enabled = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("view.angle_readout", "`enabled` must be a boolean"))?;
    let reply = crate::schema::view_navigation::AngleReadoutResult { enabled, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn next_tab(_s: &mut Session, _p: &Value) -> Result<Value> {
    tab_step(1)
}

fn previous_tab(_s: &mut Session, _p: &Value) -> Result<Value> {
    tab_step(-1)
}

/// The engine holds one design: the window, which holds the tabs, makes the step (#8565).
fn tab_step(step: i8) -> Result<Value> {
    serde_json::to_value(TabStep { step, param_warnings: Vec::new() }).map_err(|e| crate::EngineError::Other(e.to_string()))
}

#[cfg(test)]
mod ambient_occlusion_tests {
    use super::*;

    #[test]
    fn angle_readout_validates_display_setting_without_editing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for enabled in [true, false] {
            assert_eq!(s.execute("view.angle_readout", &json!({"enabled": enabled})).unwrap(), json!({"enabled": enabled}));
        }
        for p in [json!({}), json!({"enabled": 1}), json!({"enabled": "true"}), json!({"enabled": null}), json!({"enabled": f64::NAN})] {
            assert!(s.execute("view.angle_readout", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
        assert!(s.camera.is_none(), "a display setting leaves the view where it is");
    }
    #[test]
    fn validates_display_setting_without_editing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for enabled in [true, false] {
            assert_eq!(s.execute("view.ambient_occlusion", &json!({"enabled": enabled})).unwrap(), json!({"enabled": enabled}));
        }
        for p in [json!({}), json!({"enabled": 1}), json!({"enabled": "true"}), json!({"enabled": null})] {
            assert!(s.execute("view.ambient_occlusion", &p).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }

    #[test]
    fn perspective_is_a_display_setting_without_editing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for enabled in [true, false] {
            assert_eq!(s.execute("view.perspective", &json!({"enabled": enabled})).unwrap(), json!({"enabled": enabled}));
        }
        for p in [json!({}), json!({"enabled": 1}), json!({"enabled": "true"}), json!({"enabled": null})] {
            assert!(s.execute("view.perspective", &p).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }
}

/// One preset for contact shading, tessellation and supersampling. MSAA stays independent.
fn graphics_preset(_s: &mut Session, p: &Value) -> Result<Value> {
    let preset = p.get("preset").and_then(Value::as_str);
    let enabled = match preset {
        Some("quality") => true,
        Some("performance") => false,
        _ => return Err(bad("view.graphics_preset", "preset must be performance or quality")),
    };
    Ok(json!({"preset": preset, "enabled": enabled, "display_quality": if enabled { "fine" } else { "normal" },
        "supersampling": if enabled { 2.0 } else { 1.0 }, "supersampling_adaptive": true}))
}

#[cfg(test)]
mod graphics_preset_tests {
    use super::*;

    #[test]
    fn presets_validate_without_changing_design_or_history() {
        let mut s = Session::default();
        let before = s.doc.to_json();
        for (preset, enabled) in [("quality", true), ("performance", false)] {
            assert_eq!(
                s.execute("view.graphics_preset", &json!({"preset":preset})).unwrap(),
                json!({"preset":preset,"enabled":enabled,"display_quality":if enabled { "fine" } else { "normal" },
                    "supersampling":if enabled { 2.0 } else { 1.0 },"supersampling_adaptive":true})
            );
        }
        for params in [json!({}), json!({"preset":"fast"}), json!({"preset":true}), json!({"preset":null})] {
            assert!(s.execute("view.graphics_preset", &params).is_err());
        }
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }
}

#[cfg(test)]
#[test]
fn history_layout_validates_geometry_without_changing_the_design() {
    let mut s = Session::default();
    let doc = s.doc.clone();
    let request = json!({"width":240.0,"dock":"under_browser","collapsed":true});
    assert_eq!(s.execute("view.history_layout", &request).unwrap(), request);
    for p in [json!({"width":-1}), json!({"width":1e20}), json!({"width":"wide"}), json!({"dock":"nowhere"}), json!({"collapsed":42})] {
        assert!(s.execute("view.history_layout", &p).is_err());
    }
    assert_eq!(s.doc, doc);
    assert!(s.undo.is_empty());
}

#[cfg(test)]
#[test]
fn workspace_layout_validates_modes_without_mutating_the_design() {
    let mut session = Session::default();
    let before = session.doc.to_json();
    for mode in ["floating", "compact"] {
        assert_eq!(session.execute("view.layout", &json!({"mode":mode})).unwrap(), json!({"mode":mode}));
    }
    for params in [json!({}), json!({"mode":"wide"}), json!({"mode":true}), json!({"mode":null}), json!({"mode":42})] {
        assert!(session.execute("view.layout", &params).is_err());
    }
    assert_eq!(session.doc.to_json(), before);
    assert!(session.undo.is_empty());
}

#[cfg(test)]
#[test]
fn render_workspace_focus_and_browser_validate_without_side_effects() {
    let mut session = Session { camera: Some(Camera::default()), ..Default::default() };
    let camera = session.camera;
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for (id, field) in [("view.render_focus", "on"), ("view.render_browser", "open")] {
        let command = crate::find_command(id).unwrap();
        assert!(command.contract.is_some() && command.output.is_some());
        for on in [true, false] {
            let request = json!({field:on});
            assert_eq!(session.execute(id, &request).unwrap(), request);
        }
        for params in [
            Value::Null,
            json!([]),
            json!({}),
            json!({field:null}),
            json!({field:"true"}),
            json!({field:1e300}),
            json!({field:[]}),
            json!({field:true,"unknown":false}),
        ] {
            assert!(session.execute(id, &params).is_err(), "{id}: {params}");
        }
    }
    assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
    assert_eq!(session.camera, camera);
    assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
}

#[cfg(test)]
#[test]
fn render_workspace_shots_strip_is_a_validated_headless_preference_request() {
    let mut session = Session::default();
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for visible in [false, true] {
        assert_eq!(session.execute("view.shots_strip", &json!({"visible":visible})).unwrap(), json!({"visible":visible}));
    }
    for params in [
        Value::Null,
        json!({}),
        json!([]),
        json!({"visible":null}),
        json!({"visible":"yes"}),
        json!({"visible":1e300}),
        json!({"visible":true,"unknown":true}),
    ] {
        assert!(session.execute("view.shots_strip", &params).is_err());
    }
    assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
    assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
}

#[cfg(test)]
#[test]
fn render_frame_validates_hostile_requests_without_saving_guides_or_evaluating() {
    let mut s = Session::default();
    let document = s.doc.clone();
    let before = (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty());
    for mode in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        s.param_check = mode;
        for aspect in ["16:9", "3:2", "1:1", "4:5", "9:16", "custom"] {
            let request = json!({"aspect":aspect,"thirds":true,"safe_area":false,"centre_mark":true});
            assert_eq!(s.execute("view.render_frame", &request).unwrap(), request);
        }
        for request in [json!({"thirds":false}), json!({"safe_area":true}), json!({"centre_mark":false})] {
            assert_eq!(s.execute("view.render_frame", &request).unwrap(), request);
        }
        for request in [
            Value::Null,
            json!({}),
            json!([]),
            json!({"thirds":null}),
            json!({"thirds":1e300}),
            json!({"safe_area":"yes"}),
            json!({"centre_mark":[]}),
            json!({"aspect":"4:3"}),
            json!({"aspect":"x".repeat(1025)}),
            json!({"aspect":null}),
            json!({"thirds":true,"extra":1}),
        ] {
            assert!(s.execute("view.render_frame", &request).is_err(), "{request}");
        }
    }
    assert!(std::sync::Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.undo.len(), s.redo.len(), s.revision, s.evaluations, s.is_dirty()));
    let command = crate::find_command("view.render_frame").unwrap();
    assert!(command.contract.is_some() && command.output.is_some());
}

#[cfg(test)]
#[test]
fn render_workspace_stage_validates_without_document_changes_or_evaluation() {
    let mut session = Session::default();
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for stage in ["look", "light", "frame", "output"] {
        assert_eq!(session.execute("view.render_stage", &json!({"stage":stage})).unwrap(), json!({"stage":stage}));
    }
    for params in [
        Value::Null,
        json!({}),
        json!([]),
        json!({"stage":true}),
        json!({"stage":1e300}),
        json!({"stage":"camera"}),
        json!({"stage":"look","extra":true}),
    ] {
        assert!(session.execute("view.render_stage", &params).is_err());
    }
    assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
    assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
}

#[cfg(test)]
#[test]
fn render_workspace_dock_validates_without_mutation() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    session.execute("edit.undo", &json!({})).unwrap();
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty(), session.camera);
    for panel in ["inspector", "browser"] {
        for side in ["left", "right"] {
            let params = json!({"panel":panel,"side":side});
            assert_eq!(session.execute("view.render_dock", &params).unwrap(), params);
        }
    }
    for params in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"panel":"inspector"}),
        json!({"side":"left"}),
        json!({"panel":true,"side":"left"}),
        json!({"panel":"inspector","side":null}),
        json!({"panel":"inspector","side":1e300}),
        json!({"panel":"other","side":"left"}),
        json!({"panel":"inspector","side":"top"}),
        json!({"panel":"browser","side":"left","unknown":true}),
    ] {
        assert!(session.execute("view.render_dock", &params).is_err(), "{params}");
    }
    assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
    assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty(), session.camera));
}

#[cfg(test)]
#[test]
fn render_browser_focus_validates_without_document_changes_or_evaluation() {
    let mut session = Session::default();
    let doc = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for request in [
        json!({"kind":"nothing"}),
        json!({"kind":"environment"}),
        json!({"kind":"wear"}),
        json!({"kind":"lighting"}),
        json!({"kind":"light","id":"sun:0"}),
        json!({"kind":"shot","id":"Hero"}),
        json!({"kind":"output"}),
        json!({"kind":"sampling"}),
        json!({"kind":"effects"}),
        json!({"kind":"check"}),
    ] {
        assert_eq!(session.execute("view.render_select", &request).unwrap(), request);
    }
    for request in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"kind":"wear","id":"extra"}),
        json!({"kind":"wear","extra":true}),
        json!({"kind":"wear","id":1e300}),
        json!({"kind":"object"}),
        json!({"kind":"shot"}),
        json!({"kind":"light","id":1}),
        json!({"kind":"light","id":" "}),
        json!({"kind":"shot","id":"x".repeat(1025)}),
        json!({"kind":"output","id":"extra"}),
        json!({"kind":"sampling","id":"extra"}),
        json!({"kind":"effects","id":"extra"}),
        json!({"kind":"check","id":"extra"}),
        json!({"kind":"lighting","id":"extra"}),
        json!({"kind":"environment","extra":true}),
    ] {
        assert!(session.execute("view.render_select", &request).is_err(), "{request}");
    }
    assert!(std::sync::Arc::ptr_eq(&doc, &session.doc));
    assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
    let description = session.execute("engine.describe", &json!({"command":"view.render_select"})).unwrap();
    for key in ["input", "output"] {
        assert!(description[key].is_object());
        assert!(description[key].to_string().contains("\"lighting\""));
    }
}

#[cfg(test)]
#[test]
fn render_select_wear_schema_and_strict_contract_agree() {
    let mut session = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
    let spec = crate::find_command("view.render_select").unwrap();
    let contract = spec.contract.unwrap();
    assert_eq!(contract.allowed_keys().iter().map(String::as_str).collect::<Vec<_>>(), ["id", "kind"]);
    let input = contract.input_schema().to_value();
    let output = spec.output.unwrap()().to_value();
    for schema in [input, output] {
        assert!(schema["oneOf"][0]["properties"]["kind"]["enum"].as_array().unwrap().contains(&json!("wear")));
        assert_eq!(schema["oneOf"][0]["additionalProperties"], false);
    }
    let request = json!({"kind":"wear"});
    assert!(contract.check(&request).is_empty());
    assert_eq!(session.execute("view.render_select", &request).unwrap(), request);
    for request in [json!({"kind":"wear","id":"extra"}), json!({"kind":"wear","extra":true}), json!({"kind":"wear","id":1e300})] {
        assert!(!contract.check(&request).is_empty());
        assert!(session.execute("view.render_select", &request).is_err());
    }
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert_eq!(session.evaluations, 0);
}

#[cfg(test)]
#[test]
fn render_browser_controls_validate_without_document_changes_or_evaluation() {
    let mut session = Session::default();
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for request in [json!({"by_look":true}), json!({"search":true,"query":"Copper"}), json!({"query":""})] {
        assert_eq!(session.execute("view.render_browser", &request).unwrap(), request);
    }
    for request in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"by_look":null}),
        json!({"by_look":1}),
        json!({"search":null,"extra":true}),
        json!({"query":"x".repeat(1025)}),
    ] {
        assert!(session.execute("view.render_browser", &request).is_err());
    }
    assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
    assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
}

#[test]
fn render_inspector_visibility_is_validated_without_mutation() {
    let mut session = Session::default();
    let before = session.doc.to_json();
    let revision = session.revision;
    for visible in [false, true] {
        assert_eq!(session.execute("view.render_inspector", &json!({"visible":visible})).unwrap(), json!({"visible":visible}));
    }
    for value in [json!({}), json!({"visible":null}), json!({"visible":"false"}), json!({"visible":true,"extra":1})] {
        assert!(session.execute("view.render_inspector", &value).is_err());
    }
    assert_eq!(session.doc.to_json(), before);
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
}
