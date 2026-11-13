//! FreeCAD AI Bridge's tool set (the same 107 tool names and JSON Schemas, with descriptions
//! written here) over SolidCraft's own MCP server, so an AI client written for FreeCAD's bridge
//! drives SolidCraft unchanged.
//!
//! The names are a second vocabulary, not a second transport: every call arrives through the
//! MCP server like any other tool and goes out through [`Backend::call`], so it inherits the
//! server's token, its path allowances and its connection to the running app. The bridge is off
//! unless asked for ([`crate::Options::freecad`]).
//!
//! Each tool runs SolidCraft commands, and the ones that need a window run the control
//! channel's view, render and preference methods, which a backend with no app refuses saying so.
//! See docs/ai-bridge.md for the mapping and its limits.

mod tools_app;
mod tools_mesh;
mod tools_model;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::backend::Backend;

/// The tool definitions: the 107 names and JSON Schemas an AI client written for FreeCAD's
/// AIBridge (`aibridge_tools`) calls by, which is what makes the vocabulary compatible. Every
/// description is our own, written against the behaviour each tool has here.
const TOOLS_JSON: &str = include_str!("tools.json");

/// What a tool returns instead of a value: FreeCAD's ToolError (code, message).
#[derive(Debug)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
}

pub type R = Result<Value, ToolError>;

pub fn te(code: &'static str, message: impl Into<String>) -> ToolError {
    ToolError { code, message: message.into() }
}

/// Session state the FreeCAD tools need and SolidCraft has no place for.
#[derive(Default)]
pub struct State {
    /// Per-body placement (SolidCraft bakes moves into the timeline; FreeCAD objects carry one).
    pub place: BTreeMap<String, [f64; 16]>,
    /// Spreadsheets: sheet → cell → parameter name (cells are design parameters).
    pub sheets: BTreeMap<String, BTreeMap<String, String>>,
    /// Components made by pd_create_body (PartDesign bodies) and assembly_create.
    pub bodies: Vec<String>,
    pub assemblies: Vec<String>,
    pub active_body: Option<String>,
    /// Preferences in groups SolidCraft has no setting for.
    pub params: BTreeMap<String, BTreeMap<String, Value>>,
    /// Console lines printed by console_print, with their level.
    pub console: Vec<(String, String)>,
    /// Graphics settings before display_refine.
    pub display_backup: Option<Value>,
}

/// One call's context: the backend every tool talks through, and the state FreeCAD's tools need
/// that SolidCraft has no place for.
pub struct Cx<'a> {
    pub b: &'a mut dyn Backend,
    pub st: &'a mut State,
}

impl Cx<'_> {
    /// Run a SolidCraft command, through the control channel's `engine.execute`. A command id is
    /// not a method of the channel: its methods are a fixed set (`engine.execute`,
    /// `document.inspect`, `ui.*`), so a command id sent as one comes back `unknown method`.
    pub fn cmd(&mut self, cmd: &str, params: Value) -> Result<Value, String> {
        self.b.call("engine.execute", json!({"command": cmd, "params": params}))
    }
    /// [`Cx::cmd`], with a failure as the tool error a client sees.
    pub fn run(&mut self, cmd: &str, params: Value) -> R {
        self.cmd(cmd, params).map_err(|e| te("command_failed", format!("{cmd}: {e}")))
    }
    /// Run a control-channel method (`ui.camera`, `ui.render`, `ui.prefs`...). The same
    /// dispatcher: a backend bridged to the running app passes it on, and one with no window
    /// says so rather than appearing to work.
    pub fn ctl(&mut self, method: &str, params: Value) -> R {
        if !self.b.has_ui() {
            return Err(te("command_failed", format!("{method}: needs the running app ({})", self.b.describe())));
        }
        self.b.call(method, params).map_err(|e| te("command_failed", format!("{method}: {e}")))
    }
    /// Leave an open sketch (FreeCAD's tools work outside sketch editing). A design with no
    /// sketch open refuses this, which is the same as nothing to leave.
    pub fn leave_sketch(&mut self) {
        let _ = self.cmd("sketch.finish", json!({}));
    }
    /// Every open design and which one is active, as `ui.documents` reports them.
    pub fn docs(&mut self) -> Result<(Vec<Value>, usize), ToolError> {
        let open = self.ctl("ui.documents", json!({}))?;
        let tabs = open.get("documents").and_then(Value::as_array).cloned().unwrap_or_default();
        let active = open.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
        Ok((tabs, active))
    }
    /// Preferences: all of them for `{}`, or set the ones given, as `ui.prefs` takes them.
    pub fn prefs(&mut self, set: Value) -> R {
        self.ctl("ui.prefs", set)
    }
    /// The `doc` argument: the active design, or switch to the open one with that name.
    pub fn use_doc(&mut self, doc: Option<&str>) -> Result<(), ToolError> {
        let Some(name) = doc.filter(|d| !d.is_empty()) else { return Ok(()) };
        let (tabs, active) = self.docs()?;
        if tabs.get(active).and_then(|t| t.get("name")).and_then(Value::as_str) == Some(name) {
            return Ok(());
        }
        match tabs.iter().position(|t| t.get("name").and_then(Value::as_str) == Some(name)) {
            Some(i) => {
                self.ctl("ui.documents", json!({"action": "switch", "index": i}))?;
                Ok(())
            }
            None => Err(te("no_document", format!("Document '{name}' not found"))),
        }
    }
}

/// The schema of every tool (MCP `tools/list` entries).
pub fn tools() -> Vec<Value> {
    let mut list: Vec<Value> = serde_json::from_str(TOOLS_JSON).unwrap_or_default();
    for t in &mut list {
        let name = t.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(note) = note(&name) {
            let d = t.get("description").and_then(Value::as_str).unwrap_or("").to_string();
            t["description"] = json!(format!("{d}\n\nSolidCraft: {note}"));
        }
        if let Some(o) = t.as_object_mut() {
            o.remove("category");
        }
    }
    list
}

/// Where SolidCraft behaves differently from FreeCAD, said in the tool's description.
fn note(name: &str) -> Option<&'static str> {
    Some(match name {
        "freecad_python_exec" => {
            "SolidCraft has no Python: code is a SolidCraft command script — one command per line (`solid.box {\"length\": 20, \"width\": 10, \"height\": 5}`), or JSON ([{command, params}] or {commands: [...]}). stdout lists each command's result."
        }
        "freecad_python_eval" => "Evaluates a SolidCraft expression over the design's parameters, with units (`2 * width + 5 mm`).",
        "ss_create" | "ss_set" | "ss_get" | "ss_set_alias" => {
            "Spreadsheet cells are design parameters (named <sheet>_<cell>, or the alias), so features can use them in expressions; =expressions may use other cells."
        }
        "pd_create_body" | "pd_active_body" => "A PartDesign body is a SolidCraft component.",
        "assembly_create" | "assembly_add_part" => "An assembly is a component; added parts' bodies move into it.",
        "part_compound" => "A compound is a component holding the objects' bodies.",
        "bool_fuse" | "bool_cut" | "bool_common" => "Works on copies: the result is a new body, the inputs stay (hidden), as in FreeCAD.",
        "transform_set_placement" | "transform_translate" | "transform_rotate" => {
            "SolidCraft records moves as timeline features; placements are tracked for the session (bodies start at identity). Angles in degrees."
        }
        "mesh_to_shape" | "mesh_rebuild_cad" => {
            "Runs Convert Mesh, which rebuilds closed constant-section mechanical meshes as planar and cylindrical faces."
        }
        "mesh_loft_solid" | "mesh_to_solid_multi_axis" => {
            "Slices the mesh, makes a sketch per section and lofts them. A section is the convex outline of the slice, so a \
             waist or any other hollow in the outline is filled in. The loft is smooth: Loft has no ruled setting, so \
             `loft_ruled` and `loft_max_degree` are not used. Sections that differ sharply from one to the next, as a \
             scanned shape's do, make a surface the kernel's check can refuse; more of them (`n_slices`, `resolution_mm`) \
             is what narrows the gaps."
        }
        "shape_to_mesh" => "Makes a mesh body copy (via STL); deflections are not used.",
        "io_import" | "io_export" | "doc_open" => {
            "Formats: .solidcraft/.json designs, STEP, IGES, FreeCAD .FCStd and Rhino .3dm import, 3MF/STL/OBJ/glTF/.plasticity meshes; export STL/OBJ/STEP/3MF."
        }
        "view_screenshot" => "Renders offscreen as PNG (JPG/BMP and background colours are not supported).",
        "workbench_list" | "workbench_active" | "workbench_activate" => "SolidCraft's workspaces: DesignWorkspace and RenderWorkspace.",
        "gui_list_commands" | "gui_run_command" | "gui_command_info" => {
            "SolidCraft command ids (solid.box, solid.fillet, view…); interactive ones then wait for the user."
        }
        "display_refine" | "display_restore" => "Sets SolidCraft's graphics quality preset (anti-aliasing applies on restart).",
        "direct_push_pull" => "Planar faces use Offset Face, others Press Pull.",
        "measure_distance" => "Vertex references measure from the nearest edge end.",
        _ => return None,
    })
}

/// Check the arguments against the tool's schema as Python's tool(**arguments) would.
fn check_args(name: &str, args: &Value) -> Result<(), ToolError> {
    let list: Vec<Value> = serde_json::from_str(TOOLS_JSON).unwrap_or_default();
    let Some(t) = list.iter().find(|t| t.get("name").and_then(Value::as_str) == Some(name)) else {
        return Err(te("unknown_tool", format!("Unknown tool: {name}")));
    };
    let Some(args) = args.as_object() else { return Err(te("type_error", format!("{name}() arguments must be an object"))) };
    let schema = &t["inputSchema"];
    let props = schema.get("properties").and_then(Value::as_object);
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        for k in args.keys() {
            if !props.is_some_and(|p| p.contains_key(k)) {
                return Err(te("type_error", format!("{name}() got an unexpected keyword argument '{k}'")));
            }
        }
    }
    for r in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
        if let Some(k) = r.as_str()
            && args.get(k).is_none_or(Value::is_null)
        {
            return Err(te("type_error", format!("{name}() missing required argument: '{k}'")));
        }
    }
    Ok(())
}

/// Whether this is one of FreeCAD's tool names, so the server knows to route it here.
pub fn is_tool(name: &str) -> bool {
    names().iter().any(|n| n == name)
}

/// FreeCAD's tool names, read once from the schemas.
fn names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let list: Vec<Value> = serde_json::from_str(TOOLS_JSON).unwrap_or_default();
        list.iter().filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string)).collect()
    })
}

/// The state FreeCAD's tools keep between calls -- placements, spreadsheets, console lines --
/// for the life of the server process. One agent talks to one server, so one set of them.
fn shared_state() -> &'static std::sync::Mutex<State> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<State>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| std::sync::Mutex::new(State::default()))
}

/// Run one FreeCAD tool, as the MCP server's dispatcher calls it. A tool that fails comes back
/// as its code and message, which is what the model is shown.
///
/// A panic in an earlier call poisons the lock; the state it left is still the best we have, and
/// refusing every later call would be worse than carrying on with it.
pub fn run(b: &mut dyn Backend, name: &str, args: &Value) -> Result<Value, String> {
    let mut state = match shared_state().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    call(b, &mut state, name, args).map_err(|e| format!("{}: {}", e.code, e.message))
}
/// Run one tool by name. `state` lives as long as the connection: FreeCAD's tools remember
/// placements, spreadsheets and console lines between calls.
pub fn call(b: &mut dyn Backend, state: &mut State, name: &str, args: &Value) -> R {
    check_args(name, args)?;
    let mut cx = Cx { b, st: state };
    if let Some(r) = tools_app::call(&mut cx, name, args) {
        return r;
    }
    if let Some(r) = tools_model::call(&mut cx, name, args) {
        return r;
    }
    if let Some(r) = tools_mesh::call(&mut cx, name, args) {
        return r;
    }
    Err(te("unknown_tool", format!("Unknown tool: {name}")))
}

// ---------- argument helpers ----------
pub fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str).filter(|x| !x.is_empty())
}
pub fn f(a: &Value, k: &str, d: f64) -> f64 {
    a.get(k).and_then(Value::as_f64).unwrap_or(d)
}
pub fn b(a: &Value, k: &str, d: bool) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(d)
}
pub fn req<'a>(a: &'a Value, k: &str) -> Result<&'a str, ToolError> {
    s(a, k).ok_or_else(|| te("type_error", format!("missing required argument: '{k}'")))
}
/// `{x, y, z}` (z optional) as an array.
pub fn xyz(v: Option<&Value>, d: [f64; 3]) -> [f64; 3] {
    let g = |k: &str, i: usize| v.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(d[i]);
    [g("x", 0), g("y", 1), g("z", 2)]
}
pub fn obj_xyz(p: [f64; 3]) -> Value {
    json!({"x": p[0], "y": p[1], "z": p[2]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_freecad_tool_is_listed_once_with_a_schema() {
        let t = tools();
        assert_eq!(t.len(), 107);
        let mut names: Vec<&str> = t.iter().filter_map(|t| t["name"].as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 107);
        assert!(t.iter().all(|t| t["inputSchema"].is_object() && t["description"].is_string()));
    }

    #[test]
    fn arguments_are_checked_like_python_keyword_arguments() {
        assert!(check_args("part_box", &json!({"length": 1})).is_ok());
        assert_eq!(check_args("part_box", &json!({"lenght": 1})).unwrap_err().code, "type_error");
        assert_eq!(check_args("bool_cut", &json!({"base": "A"})).unwrap_err().code, "type_error");
        assert_eq!(check_args("no_such_tool", &json!({})).unwrap_err().code, "unknown_tool");
    }
    /// A backend that answers like the app's control channel: it knows a fixed set of methods,
    /// and anything else is unknown. It keeps what it was asked, a command id inside
    /// `engine.execute` written as `engine.execute <command>`.
    #[derive(Default)]
    struct Recorder {
        seen: Vec<String>,
    }
    impl Backend for Recorder {
        fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
            self.seen.push(match p.get("command").and_then(Value::as_str) {
                Some(c) if method == "engine.execute" => format!("engine.execute {c}"),
                _ => method.to_string(),
            });
            match method {
                "engine.execute" | "document.inspect" => Ok(json!({})),
                other => Err(format!("unknown method `{other}`")),
            }
        }
        fn has_ui(&self) -> bool {
            false
        }
        fn describe(&self) -> String {
            "a test backend".into()
        }
    }

    #[test]
    fn a_command_goes_out_inside_engine_execute_not_as_a_method() {
        // The control channel's methods are a fixed set (`engine.execute`, `document.inspect`,
        // `ui.*`), so a command id sent as a method comes back `unknown method` and the tool
        // does nothing at all. Every command the bridge runs goes out inside `engine.execute`.
        let (mut b, mut st) = (Recorder::default(), State::default());
        let _ = call(&mut b, &mut st, "part_box", &json!({"length": 1, "width": 2, "height": 3}));
        assert!(b.seen.iter().any(|m| m == "engine.execute solid.box"), "part_box never ran solid.box; asked: {:?}", b.seen);
        assert!(!b.seen.iter().any(|m| m == "solid.box"), "a command id was sent as a method: {:?}", b.seen);

        // The same for the reverse-engineering tools, which reach the mesh through a command.
        let (mut b, mut st) = (Recorder::default(), State::default());
        let _ = call(&mut b, &mut st, "mesh_loft_solid", &json!({"mesh_object": "M"}));
        assert!(
            !b.seen.iter().any(|m| m.contains('.') && !m.starts_with("engine.") && !m.starts_with("document.") && !m.starts_with("ui.")),
            "{:?}",
            b.seen
        );
    }

    #[test]
    fn the_bridge_is_off_unless_asked_and_shadows_no_tool_of_ours() {
        let ours = crate::tool_definitions_with(crate::Options::default());
        let our_names: Vec<&str> = ours.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(!our_names.contains(&"doc_new"), "off by default");
        assert!(!is_tool("inspect_design"), "one of ours is not routed to the bridge");
        assert!(is_tool("doc_new"), "a FreeCAD name is");

        let both = crate::tool_definitions_with(crate::Options { compact: false, freecad: true });
        assert_eq!(both.len(), ours.len() + 107, "asking for it adds FreeCAD's 107 and nothing else");
        for n in names() {
            assert!(!our_names.contains(&n.as_str()), "FreeCAD's `{n}` would shadow one of our own tools");
        }
    }
}
