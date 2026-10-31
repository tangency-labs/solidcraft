//! MCP tool definitions and dispatch. Every tool is a thin wrapper over engine commands
//! (`engine.execute`) and the inspection / rendering control methods.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value, json};
use solidcraft_engine::io::vfs;
use solidcraft_engine::timing;

use crate::backend::{Backend, STILL_RUNNING};

/// Most commands in one `batch` call.
const MAX_BATCH: usize = 10_000;
/// Largest image returned by `screenshot`.
const MAX_IMAGE_BYTES: usize = 32 << 20;
/// Relative tolerance on an expected volume (`expect.tol`) when the call gives none.
const EXPECT_VOLUME_TOL: f64 = 0.002;
/// Absolute tolerance on each expected bounding-box coordinate, mm.
const EXPECT_BBOX_TOL_MM: f64 = 0.05;

const VIEWS: &[&str] = &["iso", "home", "front", "back", "top", "bottom", "left", "right", "fit"];
/// What `check_parameters` can report.
const REPORTS: &[&str] = &["measure", "cylinders", "errors"];

fn obj(props: Value, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props, "additionalProperties": false});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}

/// What a tool may do, for the client's approval prompts (MCP tool annotations). Clients that
/// auto-approve on these hints must not be told less than the truth (#205).
#[derive(Clone, Copy)]
enum Effect {
    /// Only looks.
    ReadOnly,
    /// Changes the design in memory, undoably.
    Edits,
    /// Replaces the design in memory (unsaved changes are lost).
    Replaces,
    /// May read, write, overwrite or delete files on disk (any command, a file path).
    Files,
}

fn tool(name: &str, title: &str, desc: &str, schema: Value, effect: Effect) -> Value {
    let (read_only, destructive, open_world) = match effect {
        Effect::ReadOnly => (true, false, false),
        Effect::Edits => (false, false, false),
        Effect::Replaces => (false, true, false),
        Effect::Files => (false, true, true),
    };
    json!({
        "name": name,
        "title": title,
        "description": desc,
        "inputSchema": schema,
        "annotations": {"title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only, "openWorldHint": open_world},
    })
}

fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

/// The `expect` argument of `execute` and `batch`: what the design should measure once the
/// call has gone through.
fn expect_schema() -> Value {
    json!({
        "type": "object",
        "description": "Check the design after the call against the numbers you computed: {volume_mm3?, tol? (relative, default 0.002), \
                        bodies?, bbox?: {min: [x,y,z], max: [x,y,z]} (each coordinate within 0.05 mm)}. The reply gains `expectation`: \
                        {ok, measured: {volume_mm3, bodies, bbox}, expected, mismatch: [\"volume: measured X, expected Y (+Z %)\", ...]}. \
                        A mismatch is a tool error that names it first; the design is kept as built, so inspect it and fix it or carry on",
        "properties": {
            "volume_mm3": {"type": "number", "description": "Total volume of the closed bodies, mm³"},
            "tol": {"type": "number", "description": "Relative tolerance on the volume (default 0.002)"},
            "bodies": {"type": "integer", "description": "Number of bodies"},
            "bbox": {
                "type": "object",
                "description": "Bounding box of all bodies together, mm, each coordinate within 0.05 mm",
                "properties": {
                    "min": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                    "max": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                },
                "required": ["min", "max"],
                "additionalProperties": false,
            },
        },
        "additionalProperties": false,
    })
}

/// Every tool, with its JSON schema. The same list serves the headless and the bridged backend.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "list_commands",
            "List commands",
            "The command catalog: id, label, toolbar tab/panel, shortcut, one-line parameter docs and whether the command is \
             enabled right now. Ids are dotted and lower-case (solid.extrude, sketch.create, \
             solid.fillet, sketch.inspect, timeline.rollback). Optional case-insensitive \
             substring `filter` over id, label, tab and panel (not the parameter docs, so a command id \
             finds that command rather than every command whose docs mention it).",
            obj(json!({"filter": string("Substring to match, e.g. \"sketch\", \"fillet\", \"pattern\"")}), &[]),
            Effect::ReadOnly,
        ),
        tool(
            "describe_command",
            "Describe a command",
            "One command's parameter docs and, for commands with a typed contract (every new command, and more of the older ones as they are converted), the \
             JSON Schemas (2020-12) of its parameters (`input`) and of a successful result (`output`); null for the others.",
            obj(json!({"id": string("Command id, e.g. \"solid.extrude\"")}), &["id"]),
            Effect::ReadOnly,
        ),
        tool(
            "execute",
            "Execute a command",
            "Run one command with JSON parameters, exactly as the toolbar, palette and scripts do. Never opens a dialog. \
             Examples: sketch.create {plane: \"XY\"}; sketch.rectangle.two_point {p0: [0,0], p1: [40,30]}; sketch.circle.center \
             {center: [20,15], radius: 5}; sketch.dimension {entities: [curve id], value: \"width\"}; sketch.finish {}; solid.extrude \
             {distance: 20, operation: \"cut\"}; solid.fillet {edges: [[0,0,10]], radius: 3}; solid.box {length, \
             width, height}. Lengths are mm; expressions may use parameters and units (\"width / 2\", \"30 deg\"). Returns \
             the command's result; failures leave the design unchanged. Sketch commands that add geometry return \
             created: {points, curves, constraints, dimensions}: the ids of what they added, stable and not reused \
             after a delete (undo rewinds the counters with the design), usable right away \
             in sketch.dimension, sketch.constraint.* and sketch.delete. Feature commands return the feature's id \
             and name, every body (bodies) and the bodies the feature created, changed and removed (created_bodies, changed_bodies, removed_bodies). \
             view.pick {pixels: [[x,y]…]} answers the body, face, exact world point, normal and depth under pixels of a screenshot (read-only; docs/control-protocol.md). \
             `expect` {volume_mm3, tol, bodies, bbox} checks the design after the command (as `batch` does) and flags a mismatch.",
            obj(
                json!({
                    "command": string("Command id (see list_commands)"),
                    "params": {"type": "object", "description": "The command's parameters"},
                    "verbose": {"type": "boolean", "description": "false: omit the full feature body list and sketch ids repeated outside `created`; keep created_bodies, changed_bodies and removed_bodies (default true)"},
                    "expect": expect_schema(),
                }),
                &["command"],
            ),
            Effect::Files,
        ),
        tool(
            "batch",
            "Run several commands",
            "Run commands in order, stopping at the first error. Each step is {command, params}: an engine command id \
             (as list_commands lists them). A step may also name one of these tools with that tool's arguments as params, \
             and runs the engine command it stands for: set_parameter (parameters.change; `value` or `expression`), \
             list_commands (engine.commands, with `filter`), describe_command (engine.describe), execute ({command, params}), \
             measure (inspect.measure), \
             inspect_design (document.inspect), undo, redo, export, save, open, new_design, crash_reports; screenshot, \
             body_topology, batch and pending can't be steps. On an error the steps already run are undone (unless \
             rollback: false), so the batch is all-or-nothing; the reply names the failing step (index from 0), its error, \
             every result before it, whether the design really went back (rolled_back), the steps whose changes are in the \
             design (applied) and those never sent (not_run). The usual way to build a part: sketch, draw, dimension, \
             finish, extrude, fillet… in one call. Give `expect` {volume_mm3, tol, bodies, bbox} with the numbers you computed \
             beforehand: after the last step the design is measured and compared, so no separate measure call is needed; a \
             mismatch is flagged as an error naming it, and the design is kept as built.",
            obj(
                json!({
                    "commands": {
                        "type": "array",
                        "description": "Steps: [{\"command\": id, \"params\": {...}}, ...]",
                        "items": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]},
                    },
                    "rollback": {"type": "boolean", "description": "Undo completed steps when one fails (default true)"},
                    "coalesce_parameters": {"type": "boolean", "description": "Set each run of consecutive plain parameter edits (parameters.change or set_parameter with a name and a value, or changes) together, rebuilding the design once for the run instead of once per edit; a run that is refused goes step by step, naming the refused step. Other steps still see the design up to date. Each step keeps its own result; the run is one undo step (default false)"},
                    "timings": {"type": "boolean", "description": "Add `timings`: [{index, command, ms}] for every step run (the failing one included) and `total_ms` (default false)"},
                    "profile": {"type": "boolean", "description": "Like timings, and each step's `profile`: {boolean_ms, tessellate_ms, other_ms}, its time in kernel booleans, in tessellation and the rest (default false)"},
                    "verbose": {"type": "boolean", "description": "false: a compact reply. Feature steps leave out the full body list (created_bodies, changed_bodies and removed_bodies stay), sketch steps the ids they repeat outside `created` (default true)"},
                    "expect": expect_schema(),
                }),
                &["commands"],
            ),
            Effect::Files,
        ),
        tool(
            "inspect_design",
            "Inspect the design",
            "The whole design: user parameters (expression, value, errors), the timeline (features with type, suppressed, \
             rolled back, error, warning, timing), bodies (volume, area, centre of mass, bounding box, face/edge/vertex \
             counts), sketches (curves with ids and coordinates, points, constraints, profiles, degrees of freedom, solve \
             status), assembly joints (what each origin is attached to: occurrence, the feature, face, edge or sketch \
             curve its face names refer to, whether it resolves and why not; `execute` `joint.inspect {joint}` gives one), \
             the active sketch, selection and undo depth.",
            obj(
                json!({
                    "measure": {"type": "boolean", "description": "Measure bodies (default true)"},
                    "sketches": {"type": "boolean", "description": "Full sketch contents (default true; false = summary only)"},
                    "joints": {"type": "boolean", "description": "Joint definitions as joint.inspect gives them, when the design has joints (default true)"},
                }),
                &[],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "measure",
            "Measure bodies",
            "Volume (mm³), surface area (mm²), centre of mass, bounding box and face/edge/vertex counts of one body or all \
             bodies, with totals. With `items` (one or two selections, as `execute` takes them: faces, edges, vertices, \
             sketch curves, profiles): one item's length, area or position, a circle's, arc's or cylinder's `diameter` and \
             `radius` (with `circumference`, or an arc's `arc_length` and `angle_deg`), a slot's `slot` sizes; two items' \
             `min` and `max` distance and, where one is round, `center_distance` (centre to centre, or from its axis). \
             `summary: true` keeps one short object per body (name, volume_mm3, bbox, faces, closed) and the total volume: \
             enough to check a stage against the numbers you computed, at a fraction of the size.",
            obj(
                json!({
                    "body": string("Body name (default: all bodies)"),
                    "items": {"type": "array", "items": {"type": "object"}, "description": "One or two selections to measure instead of bodies, e.g. {\"type\": \"face\", \"body\": \"Body1\", \"index\": 3, \"point\": [x,y,z]}"},
                    "summary": {"type": "boolean", "description": "Per body only name, volume_mm3, bbox {min, max}, faces and closed, and total.volume_mm3 (default false: the full reply)"},
                }),
                &[],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "body_topology",
            "List edges and faces",
            "The edges (index, midpoint, length, endpoints) and faces (index, name, area, centroid, plane normal, a `point` that lies \
             on the face, and for a cylindrical face its `cylinder`: origin, axis, radius) of a body. Use an edge midpoint as the \
             [x,y,z] point that picks the edge for fillet and chamfer; a face's point or its name picks the face for the face commands. \
             `faces: \"cylinders\"` lists only the cylindrical faces, as `cylinders`, with the halves the kernel splits one cylinder \
             into merged: {radius, origin, axis, extent, names}, `extent` the [min, max] distance along `axis` from `origin` that \
             the cylinder spans, and no edges. That is enough to check where every hole and boss is, at a fraction of the size.",
            obj(
                json!({
                    "body": string("Body name, e.g. \"Body1\""),
                    "faces": {"type": "string", "enum": ["all", "cylinders"], "description": "\"all\" (default): every edge and face. \"cylinders\": only the cylindrical faces, merged, with their extent along the axis, and no edges"},
                }),
                &["body"],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "set_parameter",
            "Set a parameter",
            "Create or change a user parameter and re-evaluate the timeline. `value` is a number (mm) or an expression \
             (\"40 mm\", \"width * 2\", \"30 deg\"); `expression` is the same thing under the name the parameters.change command \
             uses. Give one of the two (both only when they agree). Returns the new value, the features recomputed (`recomputed`), the \
             features whose earlier result for the same inputs was reused instead (`restored`, e.g. after setting a value back) and any \
             features that now fail.",
            obj(
                json!({
                    "name": string("Parameter name"),
                    "value": {"type": ["number", "string"], "description": "Number or expression (or give it as `expression`)"},
                    "expression": {"type": ["number", "string"], "description": "Same as `value` (the parameters.change name); give one of the two"},
                    "unit": string("mm, deg or \"\" (unitless); default from the expression"),
                    "comment": string("Comment shown in the parameters table"),
                }),
                &["name"],
            ),
            Effect::Edits,
        ),
        tool(
            "check_parameters",
            "Try parameter values",
            "Try parameter values without keeping them: set every parameter in `set` with one rebuild, report what `report` \
             asks for, and put the design back exactly as it was (undo history included). `measure`: volume, bbox, bodies and \
             faces per body as the measure tool reports them; `errors`: the features that fail or warn and the parameters that \
             don't evaluate; `cylinders`: every cylindrical face as {body, radius, origin, axis, faces, name}, the halves the \
             kernel splits one cylinder into merged (origin is the axis point closest to the world origin). The reply ends with \
             `restored: true`; if the design could not be put back the call is an error with `restored: false` and the values \
             still applied. An unknown parameter name is an error and nothing changes. Replaces the loop set_parameter, \
             measure, body_topology, set_parameter back.",
            obj(
                json!({
                    "set": {
                        "type": "object",
                        "description": "Values to try, by parameter name: a number (mm) or an expression (\"140 mm\", \"wall * 2\"), as set_parameter's `value`",
                        "additionalProperties": {"type": ["number", "string"]},
                    },
                    "report": {
                        "type": "array",
                        "items": {"type": "string", "enum": REPORTS},
                        "description": "What to report: measure, cylinders, errors (default all three)",
                    },
                }),
                &["set"],
            ),
            Effect::Edits,
        ),
        tool(
            "screenshot",
            "Look at the design",
            "Render the design and return it as a PNG image. Headless: a CPU render of the model, fitted to the view. \
             Connected to the app: a screenshot of the window (source \"window\", the default) or a render of the model \
             with the app's camera (source \"model\"). Both show an active section (inspect.section) with its cut face \
             capped. `view` first turns the camera to a standard view or a saved named view (view.save), `camera` to              any camera (as view.apply). \
             `grid: false` and `axes: false` leave the ground grid (on Z 0) and the origin's axes out of a model render. \
             A body whose appearance is see-through (appearance.assign with an `opacity` below 1) is drawn see-through, \
             so what is inside it shows: that is how to look at the body a cut would remove inside the material it meets.",
            obj(
                json!({
                    "view": string(
                        "Standard view (iso, home, front, back, top, bottom, left, right, fit) or the name of a saved named view                          (default: keep the current one; headless starts at iso)"
                    ),
                    "camera": {"type": "object", "description": "Look from this camera instead: {target: [x,y,z], yaw, pitch, distance, fov?}                         (radians, mm; as view.save and view.apply)"},
                    "path": string("Also keep the PNG at this path"),
                    "width": {"type": "integer", "description": "Render width in pixels (model renders; default 960)"},
                    "height": {"type": "integer", "description": "Render height in pixels (model renders; default 600)"},
                    "source": {"type": "string", "enum": ["window", "model"], "description": "window = app screenshot, model = model render"},
                    "grid": {"type": "boolean", "description": "false: leave the ground grid out of the render. It lies on Z 0 and cuts through a part that straddles it. Model renders only: default true, or as the app shows it when connected; with `grid` or `axes` a connected screenshot is a model render, and source \"window\" is an error"},
                    "axes": {"type": "boolean", "description": "false: leave the origin's three axes out of the render (see `grid`). Axes hidden with browser.visibility stay hidden"},
                }),
                &[],
            ),
            Effect::Files,
        ),
        tool(
            "export",
            "Export",
            "Export bodies to STEP (.step/.stp), 3MF (.3mf), binary STL (.stl), ASCII STL (format stla), OBJ or experimental OSD v2. The format \
             comes from the extension unless given. URDF (.urdf) exports the whole assembly for robot simulators: a link per \
             component placement with mass and inertia, joints from the assembly's joints, and STL meshes in metres written \
             next to it (docs/urdf.md); RViz needs mesh_prefix such as package://robot/meshes/.",
            obj(
                json!({
                    "path": string("Output file"),
                    "format": {"type": "string", "enum": ["step", "3mf", "3mf-tagged", "stl", "stla", "obj", "osd", "urdf"]},
                    "bodies": {"type": "array", "items": {"type": "string"}, "description": "Body names (default: all; not with urdf)"},
                    "mesh_prefix": string("urdf: put before each mesh file name (default: none, so relative to the .urdf)"),
                    "effort": {"type": "number", "description": "urdf: joint effort limit, N or N m (default 1000)"},
                    "velocity": {"type": "number", "description": "urdf: joint velocity limit, m/s or rad/s (default 10)"},
                }),
                &["path"],
            ),
            Effect::Files,
        ),
        tool(
            "crash_reports",
            "Crash reports",
            "SolidCraft's crash reports on this computer (the app's when connected to it). Without `id`: the list, newest \
             first (id, kind, time, message, command, count, version, path, replayable). With `id`: the whole report: \
             message, location, backtrace, app version and commit, system, the command that crashed and the recent ones, \
             and design_path / replay_path when it can be replayed: `solidcraft-cli crashes replay <id>`, or \
             `solidcraft-cli run <replay_path> --in <design_path>`. Reports never leave this computer.",
            obj(
                json!({
                    "id": string("A report id from the list"),
                    "limit": {"type": "integer", "description": "Reports to list (default 20)"},
                }),
                &[],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "running",
            "What the app is running",
            "What the app is running or has queued right now, for every client: each request's connection and id, method, \
             command id (a batch's step count), seconds since it arrived (running_s) and whether it still waits its turn \
             (queued); `self` marks a request sent on the connection that asks. Answers at once, even while the app is busy, \
             and never shows parameters. Ask it when a call timed out or the app seems slow, before sending a change again.",
            obj(json!({}), &[]),
            Effect::ReadOnly,
        ),
        tool(
            "pending",
            "Result of a running request",
            "Bridged to the app: the result of a request that was still running when its tool call returned (an error \
             starting \"still running:\"). Waits for it up to 60 s: then its own result (or error), or done: false while \
             it still runs (call again). pending: null when nothing is running. A request still running was not \
             cancelled: don't send it again; other calls wait for it before they are sent.",
            obj(json!({}), &[]),
            Effect::ReadOnly,
        ),
        tool("undo", "Undo", "Undo the last change to the design.", obj(json!({}), &[]), Effect::Edits),
        tool("redo", "Redo", "Redo the last undone change.", obj(json!({}), &[]), Effect::Edits),
        tool(
            "new_design",
            "New design",
            "Start a new, empty design (unsaved changes to the current one are discarded).",
            obj(json!({"name": string("Design name (default Untitled)")}), &[]),
            Effect::Replaces,
        ),
        tool(
            "open",
            "Open a design",
            "Open a .sc design file (or a .solidcraft one from an earlier release), a STEP file (.step/.stp: a new design whose bodies come from the file, as an \
             Import base feature in a component named after the file; an assembly's parts become nested components) or a \
             mesh (.3mf/.stl/.obj/.osd/.glb/.gltf: mesh bodies in such a component; a glTF file's nodes become nested components and its \
             materials appearances); the result reports the component (id, component_name). Replaces the current design; insert into it \
             instead with the file.insert_step (STEP) or file.insert_mesh (mesh) commands. A STEP file from a system \
             that has Y up opens lying on its back: `up: \"y\"` turns its component a quarter turn about X (Y becomes Z). glTF is Y up, \
             so it is turned by default (`up: \"z\"` keeps its axes). `names` picks what the bodies are named after when the solids' own names say nothing useful.",
            obj(
                json!({
                    "path": string("Path to a .sc (or .solidcraft), .step/.stp, .3mf, .stl, .obj, .osd, .glb or .gltf file"),
                    "up": {"type": "string", "enum": ["z", "y"], "description": "STEP and glTF only. z keeps the file's axes; y: the file has Y up, so its bodies are turned to stand Z up. Default: z for STEP, y for glTF"},
                    "names": {"type": "string", "enum": ["auto", "solid", "product", "instance"], "description": "STEP, IGES, FreeCAD and Rhino. auto (default): keep meaningful solid names, replace placeholders with the nearest named product, else instance; solid: the solid's own name; product: the part's name; instance: the occurrence's name, else the product's"},
                }),
                &["path"],
            ),
            Effect::Files,
        ),
        tool(
            "save",
            "Save the design",
            "Save the design to `path` or its current file (a design opened as .solidcraft keeps that name). New designs are .sc files. The default is a design container; a .json path selects plain JSON. Set format to container or json to override the path. \
             By default the path must lie in one of the agent's folders: the folder of a design opened or saved in this session (not its hidden files), \
             SolidCraft's recovery folder, or a folder this server was started with `--allow-path DIR`. For a folder outside them, ask the user \
             to add `--allow-path` and that folder to this server's arguments in their MCP client configuration.",
            obj(
                json!({"path": string("Path to save to, e.g. part.sc (default: the current file)"), "format": {"type": "string", "enum": ["container", "json"], "description": "Design encoding (default: JSON for .json, container otherwise)"}}),
                &[],
            ),
            Effect::Files,
        ),
        tool(
            "skill_reference",
            "Skill reference",
            "Read a document of SolidCraft's bundled agent skills (solidcraft-modeling, solidcraft-mechanisms, \
             solidcraft-editing, solidcraft-checking) from the copy built into SolidCraft. `file` defaults to reference.md; \
             each skill also has SKILL.md. Unknown skills or files are refused with the available ones.",
            obj(
                json!({
                    "skill": string("Skill name, such as solidcraft-modeling"),
                    "file": {"type": "string", "description": "Document of that skill", "default": "reference.md"},
                }),
                &["skill"],
            ),
            Effect::ReadOnly,
        ),
    ]
}

fn type_matches(t: &str, v: &Value) -> bool {
    match t {
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        "boolean" => v.is_boolean(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => true,
    }
}

/// Validates `args` against the tool's schema (top level): unknown tool, missing required
/// arguments, unknown arguments, type and enum mismatches.
pub(crate) fn check_arguments(name: &str, args: &Value) -> Result<(), String> {
    let defs = tool_definitions();
    let def = defs.iter().find(|d| d["name"] == name).ok_or_else(|| format!("unknown tool `{name}`"))?;
    let schema = &def["inputSchema"];
    let Some(a) = args.as_object() else { return Err(format!("{name}: arguments must be an object")) };
    for r in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if a.get(r).is_none_or(Value::is_null) {
            return Err(format!("{name}: missing required argument `{r}`"));
        }
    }
    let props = schema["properties"].as_object();
    for (k, v) in a {
        let Some(p) = props.and_then(|p| p.get(k)) else {
            let valid: Vec<&str> = props.map(|p| p.keys().map(String::as_str).collect()).unwrap_or_default();
            let valid = if valid.is_empty() { "none".to_string() } else { valid.join(", ") };
            let k: String = k.chars().take(64).collect();
            return Err(format!("{name}: unknown argument `{k}`; its arguments are: {valid}"));
        };
        if v.is_null() {
            continue;
        }
        let ok = match &p["type"] {
            Value::String(t) => type_matches(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_matches(t, v)),
            _ => true,
        };
        if !ok {
            return Err(format!("{name}: argument `{k}` must be of type {}", p["type"]));
        }
        if let Some(allowed) = p["enum"].as_array()
            && !allowed.contains(v)
        {
            return Err(format!("{name}: `{k}` must be one of {}", Value::Array(allowed.clone())));
        }
    }
    Ok(())
}

/// A tool result: content blocks plus the error flag.
fn text_result(v: &Value, is_error: bool) -> Value {
    let text = match v {
        Value::String(s) => s.clone(),
        v => serde_json::to_string_pretty(v).unwrap_or_default(),
    };
    let mut r = json!({"content": [{"type": "text", "text": text}]});
    if is_error {
        r["isError"] = json!(true);
    }
    r
}

/// Run a tool. Failures are tool results with `isError: true` (the model sees the message);
/// nothing here panics on hostile arguments.
pub fn call_tool(b: &mut dyn Backend, name: &str, args: &Value) -> Value {
    let empty = Map::new();
    let a = args.as_object().unwrap_or(&empty);
    let r = match name {
        "screenshot" => return screenshot(b, a),
        "batch" => return batch(b, a),
        "check_parameters" => return check_parameters(b, a),
        "execute" => return execute(b, a),
        _ => dispatch(b, name, a),
    };
    match r {
        Ok(v) => text_result(&v, false),
        Err(e) => text_result(&Value::String(e), true),
    }
}

/// `execute`: one command, then its `expect` checked against the design.
fn execute(b: &mut dyn Backend, a: &Map<String, Value>) -> Value {
    let expect = match Expect::parse(a) {
        Ok(e) => e,
        Err(e) => return text_result(&Value::String(e), true),
    };
    match dispatch(b, "execute", a) {
        Ok(v) => with_expectation(b, v, expect),
        Err(e) => text_result(&Value::String(e), true),
    }
}

/// What `execute` or `batch` was told the design would measure once the call went through
/// (`expect`): the numbers an agent computes before building, compared by the server instead
/// of by hand after a separate `measure` call. Volume and body count hold whatever kernel
/// builds the design.
struct Expect {
    volume_mm3: Option<f64>,
    tol: f64,
    bodies: Option<u64>,
    bbox: Option<([f64; 3], [f64; 3])>,
}

impl Expect {
    /// The tool's `expect` argument: `None` when it was not given. A malformed one is refused
    /// before the call runs, so a typo never builds a design that then goes unchecked.
    fn parse(a: &Map<String, Value>) -> Result<Option<Expect>, String> {
        let Some(v) = a.get("expect").filter(|v| !v.is_null()) else { return Ok(None) };
        let Some(o) = v.as_object() else { return Err("expect: must be an object {volume_mm3?, tol?, bodies?, bbox?}".into()) };
        if let Some(k) = o.keys().find(|k| !["volume_mm3", "tol", "bodies", "bbox"].contains(&k.as_str())) {
            let k: String = k.chars().take(64).collect();
            return Err(format!("expect: unknown key `{k}`; its keys are: volume_mm3, tol, bodies, bbox"));
        }
        let number = |k: &str| -> Result<Option<f64>, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| format!("expect: `{k}` must be a number")),
            }
        };
        let volume_mm3 = number("volume_mm3")?;
        if volume_mm3.is_some_and(|v| v < 0.0) {
            return Err("expect: `volume_mm3` must not be negative".into());
        }
        let tol = number("tol")?.unwrap_or(EXPECT_VOLUME_TOL);
        if tol < 0.0 {
            return Err("expect: `tol` must not be negative".into());
        }
        let bodies = match o.get("bodies") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or("expect: `bodies` must be a whole number, 0 or more")?),
        };
        let bbox = match o.get("bbox") {
            None | Some(Value::Null) => None,
            Some(v) => Some(Self::parse_bbox(v)?),
        };
        if volume_mm3.is_none() && bodies.is_none() && bbox.is_none() {
            return Err("expect: nothing to check; give volume_mm3, bodies or bbox".into());
        }
        Ok(Some(Expect { volume_mm3, tol, bodies, bbox }))
    }

    fn parse_bbox(v: &Value) -> Result<([f64; 3], [f64; 3]), String> {
        let Some(o) = v.as_object() else { return Err("expect: `bbox` must be {min: [x,y,z], max: [x,y,z]} in mm".into()) };
        if let Some(k) = o.keys().find(|k| !["min", "max"].contains(&k.as_str())) {
            let k: String = k.chars().take(64).collect();
            return Err(format!("expect: unknown key `bbox.{k}`; its keys are: min, max"));
        }
        let corner = |k: &str| -> Result<[f64; 3], String> {
            let err = || format!("expect: `bbox.{k}` must be [x, y, z] in mm");
            let xyz = o.get(k).and_then(Value::as_array).filter(|a| a.len() == 3).ok_or_else(err)?;
            let mut c = [0.0; 3];
            for (slot, x) in c.iter_mut().zip(xyz) {
                *slot = x.as_f64().filter(|x| x.is_finite()).ok_or_else(err)?;
            }
            Ok(c)
        };
        let (min, max) = (corner("min")?, corner("max")?);
        if min.iter().zip(&max).any(|(lo, hi)| lo > hi) {
            return Err("expect: `bbox.min` must not exceed `bbox.max`".into());
        }
        Ok((min, max))
    }

    /// What was expected, for the reply.
    fn expected(&self) -> Value {
        let mut e = Map::new();
        if let Some(v) = self.volume_mm3 {
            e.insert("volume_mm3".into(), json!(v));
            e.insert("tol".into(), json!(self.tol));
        }
        if let Some(n) = self.bodies {
            e.insert("bodies".into(), json!(n));
        }
        if let Some((min, max)) = self.bbox {
            e.insert("bbox".into(), json!({"min": min, "max": max}));
        }
        Value::Object(e)
    }

    /// Measure the design (as the `measure` tool does) and compare: the reply's `expectation`
    /// (`ok`, `measured`, `expected`, `mismatch`).
    fn check(&self, b: &mut dyn Backend) -> Result<Value, String> {
        let m = exec(b, "inspect.measure", json!({}))?;
        let bodies = m["body_count"].as_u64().unwrap_or(0);
        let volume = m["total"]["volume_mm3"].as_f64();
        let bbox = m["bodies"].as_array().into_iter().flatten().filter_map(|body| corners(&body["bbox"])).reduce(|(lo, hi), (min, max)| {
            ([lo[0].min(min[0]), lo[1].min(min[1]), lo[2].min(min[2])], [hi[0].max(max[0]), hi[1].max(max[1]), hi[2].max(max[2])])
        });
        let mut mismatch: Vec<String> = Vec::new();
        if let Some(e) = self.volume_mm3 {
            match volume {
                Some(v) if (v - e).abs() <= self.tol * e.abs() => {}
                Some(v) => {
                    let pct = if e != 0.0 { format!(" ({:+.2} %)", (v - e) / e * 100.0) } else { String::new() };
                    mismatch.push(format!("volume: measured {}, expected {}{pct}", num(v), num(e)));
                }
                None => mismatch.push(format!("volume: not measured (no closed body), expected {}", num(e))),
            }
        }
        if let Some(e) = self.bodies
            && bodies != e
        {
            mismatch.push(format!("bodies: measured {bodies}, expected {e}"));
        }
        if let Some((emin, emax)) = self.bbox {
            let expected = format!("min {} max {}", xyz_text(emin), xyz_text(emax));
            match bbox {
                Some((min, max)) => {
                    let off: Vec<String> = [("min", min, emin), ("max", max, emax)]
                        .into_iter()
                        .flat_map(|(side, m, e)| {
                            ["x", "y", "z"]
                                .into_iter()
                                .zip(m)
                                .zip(e)
                                .filter(|&((_, m), e)| (m - e).abs() > EXPECT_BBOX_TOL_MM)
                                .map(move |((axis, m), e)| format!("{side} {axis} off by {} mm", num(m - e)))
                        })
                        .collect();
                    if !off.is_empty() {
                        mismatch.push(format!(
                            "bbox: measured min {} max {}, expected {expected} ({})",
                            xyz_text(min),
                            xyz_text(max),
                            off.join(", ")
                        ));
                    }
                }
                None => mismatch.push(format!("bbox: no body to measure, expected {expected}")),
            }
        }
        Ok(json!({
            "ok": mismatch.is_empty(),
            "measured": {"volume_mm3": volume, "bodies": bodies, "bbox": bbox.map(|(min, max)| json!({"min": min, "max": max}))},
            "expected": self.expected(),
            "mismatch": mismatch,
        }))
    }
}

/// `{min: [x,y,z], max: [x,y,z]}` as two corners.
fn corners(v: &Value) -> Option<([f64; 3], [f64; 3])> {
    let xyz = |k: &str| -> Option<[f64; 3]> {
        let a = v.get(k)?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    Some((xyz("min")?, xyz("max")?))
}

/// A number in a mismatch message: at most four decimals, no trailing zeros.
fn num(x: f64) -> String {
    let r = (x * 1e4).round() / 1e4;
    // `-0` for a coordinate a hair under zero would read as a sign error.
    format!("{}", if r == 0.0 { 0.0 } else { r })
}

fn xyz_text(c: [f64; 3]) -> String {
    format!("[{}, {}, {}]", num(c[0]), num(c[1]), num(c[2]))
}

/// A reply that went through, with its `expectation` when `expect` was given. A mismatch is a
/// tool error that names it first; the design stays as built (the agent decides what to do).
fn with_expectation(b: &mut dyn Backend, mut reply: Value, expect: Option<Expect>) -> Value {
    let Some(expect) = expect else { return text_result(&reply, false) };
    let expectation = match expect.check(b) {
        Ok(e) => e,
        Err(e) => json!({"ok": false, "expected": expect.expected(), "mismatch": [format!("the design could not be measured: {e}")]}),
    };
    let mismatch: Vec<&str> = expectation["mismatch"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let note = (expectation["ok"] != true).then(|| {
        format!(
            "expectation not met: {}. The design is kept as built: inspect it, then fix it (timeline.edit, undo) or carry on.",
            mismatch.join("; ")
        )
    });
    if !reply.is_object() {
        reply = json!({"result": reply});
    }
    reply["expectation"] = expectation;
    let Some(note) = note else { return text_result(&reply, false) };
    let mut r = text_result(&reply, true);
    if let Some(content) = r["content"].as_array_mut() {
        content.insert(0, json!({"type": "text", "text": note}));
    }
    r
}

fn exec(b: &mut dyn Backend, command: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command": command, "params": params})).map_err(|error| command_error(command, error))
}

/// Enrich an unknown-id error without retrying or changing the command that was requested.
fn command_error(command: &str, error: String) -> String {
    if !error.starts_with(&format!("unknown command `{command}`")) {
        return error;
    }
    // Bound the quadratic comparison work, including for direct callers without a line cap.
    if command.len() > 128 {
        return error;
    }
    let id = command.split(['{', '(']).next().unwrap_or(command).trim();
    let packed = id != command.trim();
    let normalize = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect::<String>();
    let normalized = normalize(id);
    if normalized.is_empty() {
        return error;
    }
    let words: Vec<_> = id.split(|c: char| !c.is_ascii_alphanumeric()).filter(|s| !s.is_empty()).collect();
    let mut closest: Vec<_> = solidcraft_engine::command_specs()
        .into_iter()
        .filter_map(|spec| {
            let candidate = normalize(spec.id);
            let distance = command_distance(&normalized, &candidate);
            let shared = spec.id.split('.').filter(|word| words.iter().any(|w| w.eq_ignore_ascii_case(word))).count();
            (distance <= (normalized.len() / 4).max(2) || shared > 0).then_some((
                normalized != candidate,
                distance,
                std::cmp::Reverse(shared),
                spec.id,
            ))
        })
        .collect();
    closest.sort_unstable();
    let hints: Vec<_> = closest.iter().take(3).map(|(_, _, _, id)| format!("`{id}`")).collect();
    let mut message = format!("unknown command `{command}`");
    if !hints.is_empty() {
        message.push_str(&format!("; closest: {}", hints.join(", ")));
    }
    if packed {
        message.push_str("; command ids have no parameters; pass them in `params`");
    }
    message
}

fn command_distance(a: &str, b: &str) -> usize {
    let mut row: Vec<usize> = (1..=b.len()).collect();
    for (i, ca) in a.bytes().enumerate() {
        let mut diagonal = i;
        let mut left = i + 1;
        for (cell, cb) in row.iter_mut().zip(b.bytes()) {
            let next = (diagonal + usize::from(ca != cb)).min(*cell + 1).min(left + 1);
            diagonal = *cell;
            *cell = next;
            left = next;
        }
    }
    row.last().copied().unwrap_or(a.len())
}

fn pick(a: &Map<String, Value>, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|k| a.get(*k).filter(|v| !v.is_null()).map(|v| (k.to_string(), v.clone()))).collect())
}

fn str_arg<'a>(a: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

fn dispatch(b: &mut dyn Backend, name: &str, a: &Map<String, Value>) -> Result<Value, String> {
    match name {
        "list_commands" => {
            let all = b.call("engine.commands", json!({}))?;
            Ok(filter_commands(all, str_arg(a, "filter")))
        }
        "describe_command" => {
            let id = str_arg(a, "id").ok_or("`id` is required")?;
            exec(b, "engine.describe", json!({"command": id}))
        }
        "execute" => {
            let id = str_arg(a, "command").ok_or("`command` is required")?;
            let v = exec(b, id, a.get("params").cloned().filter(Value::is_object).unwrap_or(json!({})))?;
            Ok(if a.get("verbose").and_then(Value::as_bool).unwrap_or(true) { v } else { compact(v) })
        }
        "inspect_design" => {
            let measure = a.get("measure").and_then(Value::as_bool).unwrap_or(true);
            let mut d = b.call("document.inspect", json!({"measure": measure}))?;
            if a.get("sketches").and_then(Value::as_bool).unwrap_or(true) {
                let ids: Vec<Value> = d["sketches"].as_array().map(|v| v.iter().map(|s| s["id"].clone()).collect()).unwrap_or_default();
                let full: Vec<Value> = ids
                    .into_iter()
                    .map(|id| exec(b, "sketch.inspect", json!({"sketch": id.clone()})).unwrap_or_else(|e| json!({"id": id, "error": e})))
                    .collect();
                d["sketches"] = Value::Array(full);
            }
            // Joints: what each is attached to (left out when there are none, or the app is older).
            if a.get("joints").and_then(Value::as_bool).unwrap_or(true)
                && let Ok(j) = exec(b, "joint.inspect", json!({}))
                && j["joints"].as_array().is_some_and(|v| !v.is_empty())
            {
                d["joints"] = j["joints"].clone();
            }
            Ok(d)
        }
        "pending" => pending(b),
        "measure" => {
            if let Some(items) = a.get("items") {
                return exec(b, "inspect.measure", json!({ "items": items }));
            }
            let p = match str_arg(a, "body") {
                Some(body) => json!({"bodies": [body]}),
                None => json!({}),
            };
            let m = exec(b, "inspect.measure", p)?;
            Ok(if a.get("summary").and_then(Value::as_bool).unwrap_or(false) { measure_summary(m) } else { m })
        }
        "body_topology" => {
            let p = pick(a, &["body"]);
            let edges = exec(b, "model.edges", p.clone())?;
            let faces = exec(b, "model.faces", p)?;
            Ok(match str_arg(a, "faces") {
                Some("cylinders") => json!({"body": a.get("body"), "cylinders": topology_cylinders(&faces["faces"], &edges["edges"])}),
                _ => json!({"body": a.get("body"), "edges": edges["edges"], "faces": faces["faces"]}),
            })
        }
        "set_parameter" => {
            let mut p = pick(a, &["name", "unit", "comment"]);
            p["expression"] = parameter_value(a)?;
            exec(b, "parameters.change", p)
        }
        "export" => exec(b, "file.export", pick(a, &["path", "format", "bodies", "mesh_prefix", "effort", "velocity"])),
        "crash_reports" => match a.get("id") {
            Some(_) => exec(b, "diagnostics.crash_report", pick(a, &["id"])),
            None => exec(b, "diagnostics.crash_reports", pick(a, &["limit"])),
        },
        "skill_reference" => crate::skill_reference::read(a),
        "running" => b.call("control.running", json!({})),
        "undo" => exec(b, "edit.undo", json!({})),
        "redo" => exec(b, "edit.redo", json!({})),
        "new_design" => exec(b, "file.new", pick(a, &["name"])),
        "open" => exec(b, "doc.open", pick(a, &["path", "up", "names"])),
        "save" => exec(b, "file.save", pick(a, &["path", "format"])),
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// The command catalog filtered by a case-insensitive substring of id, label, panel or tab
/// (`list_commands`; not the parameter docs, which name other commands, #269).
fn filter_commands(all: Value, filter: Option<&str>) -> Value {
    let Some(f) = filter.map(str::to_lowercase).filter(|f| !f.is_empty()) else { return all };
    let hit =
        |c: &Value| ["id", "label", "panel", "tab"].iter().any(|k| c.get(*k).and_then(Value::as_str).is_some_and(|s| s.to_lowercase().contains(&f)));
    Value::Array(all.as_array().map(|v| v.iter().filter(|c| hit(c)).cloned().collect()).unwrap_or_default())
}

/// `pending`: the end of a request still running when its call returned (bridged only; a
/// headless session runs every request to its end).
fn pending(b: &mut dyn Backend) -> Result<Value, String> {
    let p = b.call("control.pending", json!({}))?;
    let what = p.get("pending").cloned().unwrap_or(Value::Null);
    if what.is_null() {
        return Ok(json!({"pending": null, "note": "nothing is running"}));
    }
    let name = what.as_str().unwrap_or("the request").to_string();
    if p.get("lost").and_then(Value::as_bool) == Some(true) {
        return Err(format!(
            "the connection to the app was lost while {name} was running, and its result with it: inspect the design (inspect_design) \
             to see whether it applied before sending it again"
        ));
    }
    if p.get("done").and_then(Value::as_bool) == Some(true) {
        let reply = p.get("reply").cloned().unwrap_or(Value::Null);
        return match reply.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(json!({"pending": what, "done": true, "result": reply.get("result").cloned().unwrap_or(Value::Null)})),
            _ => Err(format!("{name} failed: {}", reply.get("error").and_then(Value::as_str).unwrap_or("no reply"))),
        };
    }
    Ok(json!({"pending": what, "done": false, "running_s": p.get("running_s"), "note": "still running: call pending again to keep waiting"}))
}

/// How a tool step's result is cut down to what the tool itself would reply.
enum Narrow {
    /// A `list_commands` step's catalog, filtered as the tool filters it.
    Commands(String),
    /// A `measure {summary: true}` step's reply, as `measure_summary` shapes it.
    MeasureSummary,
}

impl Narrow {
    fn apply(&self, v: Value) -> Value {
        match self {
            Narrow::Commands(f) => filter_commands(v, Some(f)),
            Narrow::MeasureSummary => measure_summary(v),
        }
    }
}

/// A `batch` step as the engine command it runs: an engine command as it is, or one of these
/// tools' names (an agent writes the tool it knows) with that tool's arguments as its params.
/// `Ok((command, params, narrow))`: `narrow` cuts the step's result down as the tool would; an empty
/// `command` means the step is an engine command already.
fn batch_step(step: &Value) -> Result<(String, Value, Option<Narrow>), String> {
    let command = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).ok_or("missing `command`")?;
    let empty = Map::new();
    let params = step.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}));
    let a = params.as_object().unwrap_or(&empty);
    let mapped = |c: &str, p: Value| Ok((c.to_string(), p, None));
    match command {
        "list_commands" => Ok(("engine.commands".into(), json!({}), str_arg(a, "filter").map(|f| Narrow::Commands(f.to_string())))),
        "set_parameter" => {
            let mut p = pick(a, &["name", "unit", "comment"]);
            p["expression"] = parameter_value(a)?;
            mapped("parameters.change", p)
        }
        "describe_command" => mapped("engine.describe", json!({"command": str_arg(a, "id").ok_or("a `describe_command` step needs params {id}")?})),
        "execute" => {
            let inner = str_arg(a, "command").ok_or("an `execute` step needs params {command, params}")?;
            if !inner.contains('.') && TOOL_STEPS.contains(&inner) {
                return Err(format!("an `execute` step runs an engine command, not the `{inner}` tool"));
            }
            mapped(inner, a.get("params").cloned().filter(Value::is_object).unwrap_or(json!({})))
        }
        "measure" => match a.get("items") {
            Some(items) => mapped("inspect.measure", json!({"items": items})),
            None => {
                let p = str_arg(a, "body").map_or(json!({}), |body| json!({"bodies": [body]}));
                let summary = a.get("summary").and_then(Value::as_bool).unwrap_or(false);
                Ok(("inspect.measure".into(), p, summary.then_some(Narrow::MeasureSummary)))
            }
        },
        "inspect_design" => mapped("document.inspect", json!({"measure": a.get("measure").and_then(Value::as_bool).unwrap_or(true)})),
        "undo" => mapped("edit.undo", json!({})),
        "redo" => mapped("edit.redo", json!({})),
        "export" => mapped("file.export", pick(a, &["path", "format", "bodies", "mesh_prefix", "effort", "velocity"])),
        "save" => mapped("file.save", pick(a, &["path", "format"])),
        "open" => mapped("doc.open", pick(a, &["path", "up", "names"])),
        "new_design" => mapped("file.new", pick(a, &["name"])),
        "crash_reports" => match a.get("id") {
            Some(_) => mapped("diagnostics.crash_report", pick(a, &["id"])),
            None => mapped("diagnostics.crash_reports", pick(a, &["limit"])),
        },
        "body_topology" => Err("`body_topology` is a tool, not a command: in a batch use model.edges {body} and model.faces {body}".into()),
        "screenshot" => Err("`screenshot` is a tool, not a command: call it after the batch".into()),
        "batch" => Err("batches don't nest: put the steps in this batch".into()),
        "check_parameters" => Err("`check_parameters` is a tool, not a command: call it on its own (it puts the design back itself)".into()),
        "pending" => Err("`pending` is a tool, not a command: call it on its own".into()),
        "running" => Err("`running` is a tool, not a command: call it on its own".into()),
        "skill_reference" => Err("`skill_reference` is a tool, not a command: call it on its own".into()),
        // An engine command: the step goes as it is.
        _ => Ok((String::new(), Value::Null, None)),
    }
}

/// The tool names a `batch` step may not be (or is mapped from): `execute` must not wrap them.
const TOOL_STEPS: &[&str] = &[
    "batch",
    "body_topology",
    "check_parameters",
    "crash_reports",
    "describe_command",
    "execute",
    "export",
    "inspect_design",
    "list_commands",
    "measure",
    "new_design",
    "open",
    "pending",
    "redo",
    "running",
    "save",
    "screenshot",
    "set_parameter",
    "skill_reference",
    "undo",
];

/// `set_parameter`'s new value: `value` or `expression` (the parameters.change name), one of
/// them, or both when they agree (`50` and `"50"` do).
fn parameter_value(a: &Map<String, Value>) -> Result<Value, String> {
    let text = |v: &Value| match v {
        Value::String(s) => s.trim().to_string(),
        v => v.to_string(),
    };
    match (a.get("value").filter(|v| !v.is_null()), a.get("expression").filter(|v| !v.is_null())) {
        (Some(v), None) | (None, Some(v)) => Ok(v.clone()),
        (Some(v), Some(e)) if text(v) == text(e) => Ok(v.clone()),
        (Some(_), Some(_)) => Err("set_parameter: `value` and `expression` disagree; give one of them".into()),
        (None, None) => Err("set_parameter: give the new value as `value` (or `expression`): a number (mm) or an expression".into()),
    }
}

fn undo_depth(b: &mut dyn Backend) -> Option<u64> {
    b.call("document.inspect", json!({})).ok().and_then(|d| d["undo"].as_u64())
}

/// How a failed batch goes back to where it started.
enum Undo {
    /// An engine checkpoint: the design, active sketch and undo history exactly as they were.
    Checkpoint(u64),
    /// An app without checkpoints: undo as many steps as the batch added (wrong once the undo
    /// history is full, as its depth no longer grows).
    Depth(u64),
    None,
}

impl Undo {
    fn begin(b: &mut dyn Backend) -> Undo {
        match exec(b, "edit.checkpoint", json!({})).ok().and_then(|v| v["checkpoint"].as_u64()) {
            Some(id) => Undo::Checkpoint(id),
            None => undo_depth(b).map_or(Undo::None, Undo::Depth),
        }
    }

    /// Go back: whether the design really is back where the batch started, and the number of
    /// steps undone. A failed rollback says so rather than reporting the flag the caller asked
    /// for (#3242).
    fn rollback(self, b: &mut dyn Backend, steps: usize) -> (bool, u64) {
        match self {
            Undo::Checkpoint(id) => match exec(b, "edit.restore_checkpoint", json!({"checkpoint": id})) {
                Ok(_) => (true, steps as u64),
                Err(_) => (false, 0),
            },
            Undo::Depth(before) => {
                let Some(after) = undo_depth(b) else { return (false, 0) };
                let mut undone = 0u64;
                for _ in before..after {
                    if exec(b, "edit.undo", json!({})).is_err() {
                        break;
                    }
                    undone += 1;
                }
                (undone == after.saturating_sub(before), undone)
            }
            // Nothing remembered the design; only a first step that failed left nothing behind.
            Undo::None => (steps == 0, 0),
        }
    }

    /// The batch went through: forget the checkpoint.
    fn finish(self, b: &mut dyn Backend) {
        if let Undo::Checkpoint(id) = self {
            let _ = exec(b, "edit.restore_checkpoint", json!({"checkpoint": id, "forget": true}));
        }
    }
}

/// `batch`: run steps in order, stop at the first failure and (by default) go back to the
/// design as it was before the batch. A batch that went through is then checked against its
/// `expect`.
fn batch(b: &mut dyn Backend, a: &Map<String, Value>) -> Value {
    let expect = match Expect::parse(a) {
        Ok(e) => e,
        Err(e) => return text_result(&Value::String(e), true),
    };
    match batch_run(b, a) {
        (reply, true) => text_result(&reply, true),
        (reply, false) => with_expectation(b, reply, expect),
    }
}

/// The batch itself: its reply and whether it failed.
fn batch_run(b: &mut dyn Backend, a: &Map<String, Value>) -> (Value, bool) {
    let Some(steps) = a.get("commands").and_then(Value::as_array) else {
        return (json!("`commands` must be an array of {command, params}"), true);
    };
    if steps.len() > MAX_BATCH {
        return (json!(format!("batch too long ({} steps, at most {MAX_BATCH})", steps.len())), true);
    }
    let rollback = a.get("rollback").and_then(Value::as_bool).unwrap_or(true);
    let verbose = a.get("verbose").and_then(Value::as_bool).unwrap_or(true);
    let profile = a.get("profile").and_then(Value::as_bool).unwrap_or(false);
    let timed = profile || a.get("timings").and_then(Value::as_bool).unwrap_or(false);
    // Steps that name a tool run the engine command it stands for. One that can't be a step
    // fails the batch before anything is sent.
    let mut mapped: Vec<Value> = Vec::with_capacity(steps.len());
    let mut narrows: Vec<(usize, Narrow)> = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        // The window goes to another design's tab for these, however the step names them: the
        // batch's later steps and its rollback would land in the wrong design.
        let step_result = batch_step(step).and_then(|(command, params, narrow)| {
            let id = if command.is_empty() { step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).unwrap_or("") } else { &command };
            match solidcraft_engine::find_command(id).map(|spec| spec.id) {
                Some(c @ ("component.edit_in_place" | "component.finish_edit")) => {
                    Err(format!("`{c}` goes to another design's tab: send it on its own, not in a batch"))
                }
                _ => Ok((command, params, narrow)),
            }
        });
        match step_result {
            Ok((command, _, _)) if command.is_empty() => mapped.push(step.clone()),
            Ok((command, params, narrow)) => {
                if let Some(n) = narrow {
                    narrows.push((i, n));
                }
                mapped.push(json!({"command": command, "params": params}));
            }
            // A step without a command fails where it stands, as before.
            Err(e) if e == "missing `command`" => mapped.push(step.clone()),
            Err(e) => {
                let command = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str);
                let reply = json!({
                    "ok": false,
                    "completed": 0,
                    "failed": {"index": i, "command": command, "error": e},
                    "rolled_back": false,
                    "undone": 0,
                    "applied": [],
                    "not_run": (0..steps.len()).collect::<Vec<_>>(),
                    "results": [],
                    "note": "nothing was run",
                });
                return (reply, true);
            }
        }
    }
    let steps = &mapped;
    // A step the batch would refuse at its turn refuses it now, before anything runs (#5853).
    // Only where the commands run in this process: a running app may be another version, with
    // commands this one lacks, and checks the batch against its own registry when it gets it.
    if b.in_process()
        && let Some(refusal) = solidcraft_engine::batch_preflight::check(steps, b.param_check(), &[])
    {
        let mut reply = refusal.reply(steps.len());
        if let Some(o) = reply.as_object_mut() {
            o.insert("ok".into(), Value::Bool(false));
            o.insert("applied".into(), json!([]));
        }
        if let (Some(command), Some(failed)) = (refusal.command.as_deref(), reply.get_mut("failed").and_then(Value::as_object_mut)) {
            failed.insert("error".into(), Value::String(command_error(command, refusal.error.clone())));
        }
        return (reply, true);
    }
    // A tool step's result as the tool would reply it: a `list_commands` step's catalog filtered,
    // a `measure {summary: true}` step's reply summarised.
    let narrow = |i: usize, v: Value| match narrows.iter().find(|(j, _)| *j == i) {
        Some((_, n)) => n.apply(v),
        None => v,
    };
    // An app that runs batches itself answers the whole batch in one request, so one frame
    // (#743). One too long for a control line goes step by step: the app would refuse it unrun.
    // The margin covers the envelope and the `allow_paths` the backend adds.
    let coalesce = a.get("coalesce_parameters").and_then(Value::as_bool).unwrap_or(false);
    let mut params = json!({"commands": steps, "rollback": rollback});
    if coalesce && let Some(o) = params.as_object_mut() {
        o.insert("coalesce_parameters".into(), Value::Bool(true));
    }
    if timed && let Some(o) = params.as_object_mut() {
        o.insert("timings".into(), Value::Bool(true));
        o.insert("profile".into(), Value::Bool(profile));
    }
    if params.to_string().len() + 64 * 1024 <= solidcraft_engine::MAX_LINE {
        match b.call("engine.batch", params) {
            Ok(mut r) => {
                if let Some(failed) = r.get_mut("failed")
                    && let (Some(command), Some(error)) = (failed.get("command").and_then(Value::as_str), failed.get("error").and_then(Value::as_str))
                {
                    let error = command_error(command, error.to_string());
                    if let Some(failed) = failed.as_object_mut() {
                        failed.insert("error".into(), Value::String(error));
                    }
                }
                return batch_reply(&r, steps.len(), rollback, verbose, &narrow);
            }
            // Older apps and the headless session, or the app wants the steps one by one.
            Err(e) if e.starts_with("unknown method") || e.starts_with("engine.batch unavailable") => {}
            // Never sent: no app to send it to. Or sent and still running (it was not cancelled):
            // the `pending` tool gets its result.
            Err(e) if e.starts_with(crate::backend::NOT_RUNNING) || e.starts_with(STILL_RUNNING) => {
                return (Value::String(e), true);
            }
            // The app may have run it: sending the steps again would apply them twice.
            Err(e) => {
                let msg = format!("the app did not answer engine.batch ({e}); it may have run: inspect the design before sending it again");
                return (Value::String(msg), true);
            }
        }
    }
    let start = if rollback { Undo::begin(b) } else { Undo::None };
    let mut results = Vec::with_capacity(steps.len());
    let mut rows = Vec::new();
    let clock = timing::Clock::start();
    // The kernel's share can only be measured where the commands run: in this process.
    let profile = profile && b.in_process();
    // With `coalesce_parameters`, each run of plain parameter edits as one edit (#5822); a run
    // whose combined edit is refused goes step by step.
    for run in solidcraft_engine::batch_params::plan(steps, coalesce) {
        let singles = match run {
            solidcraft_engine::batch_params::BatchRun::Step(i) => vec![i],
            solidcraft_engine::batch_params::BatchRun::Params { steps: indices, changes } => {
                let (r, took) = timing::time(profile, || exec(b, "parameters.change", solidcraft_engine::batch_params::combined_params(&changes)));
                match r {
                    Ok(v) => {
                        if timed {
                            rows.push(took.to_json(indices.first().copied().unwrap_or(0), "parameters.change"));
                        }
                        let rows = solidcraft_engine::batch_params::step_results(steps, &indices, &v);
                        results.extend(rows.into_iter().map(|v| if verbose { v } else { compact(v) }));
                        continue;
                    }
                    Err(_) => indices,
                }
            }
        };
        for i in singles {
            let Some(step) = steps.get(i) else { continue };
            let command = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str);
            let (r, took) = timing::time(profile, || match command {
                Some(id) => exec(b, id, step.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}))),
                None => Err("missing `command`".to_string()),
            });
            if timed {
                rows.push(took.to_json(i, command.unwrap_or_default()));
            }
            match r {
                Ok(v) => {
                    let v = narrow(i, v);
                    results.push(if verbose { v } else { compact(v) })
                }
                Err(e) => {
                    // A step still running in the app was not cancelled: the rollback first waits for
                    // it (the backend does), so a restored checkpoint takes it back too.
                    let running = e.starts_with(STILL_RUNNING);
                    let (went_back, undone) = start.rollback(b, i);
                    // Only a rollback asked for is reported as one.
                    let rolled_back = rollback && went_back;
                    let mut failed = json!({"index": i, "command": command, "error": e});
                    // Applied: the steps before it, unless rolled back; and the step itself while it
                    // still runs, as it will apply when it finishes.
                    let mut applied: Vec<usize> = if rolled_back { Vec::new() } else { (0..i).collect() };
                    if running && !rolled_back {
                        failed["still_running"] = json!(true);
                        applied.push(i);
                    }
                    let reply = json!({
                        "ok": false,
                        "completed": i,
                        "failed": failed,
                        "rolled_back": rolled_back,
                        "undone": undone,
                        "applied": applied,
                        "not_run": (i + 1..steps.len()).collect::<Vec<_>>(),
                        "results": results,
                    });
                    return (with_timings(reply, timed, rows, clock.ms()), true);
                }
            }
        }
    }
    start.finish(b);
    let reply = json!({"ok": true, "completed": results.len(), "results": results});
    (with_timings(reply, timed, rows, clock.ms()), false)
}

/// `reply` with `timings` and `total_ms` when they were asked for.
fn with_timings(mut reply: Value, timed: bool, rows: Vec<Value>, total_ms: f64) -> Value {
    if timed && let Some(o) = reply.as_object_mut() {
        o.insert("timings".into(), Value::Array(rows));
        o.insert("total_ms".into(), json!(timing::round_ms(total_ms)));
    }
    reply
}

/// The `batch` reply from the app's `engine.batch` answer, shaped as the per-step path shapes it.
fn batch_reply(r: &Value, len: usize, rollback: bool, verbose: bool, narrow: &dyn Fn(usize, Value) -> Value) -> (Value, bool) {
    let results: Vec<Value> = r["results"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, v)| narrow(i, v))
        .map(|v| if verbose { v } else { compact(v) })
        .collect();
    let (mut reply, error) = match r.get("failed").filter(|f| !f.is_null()) {
        None => (json!({"ok": true, "completed": results.len(), "results": results}), false),
        Some(failed) => {
            let completed = r["completed"].as_u64().map_or(results.len(), |c| c as usize).min(len);
            // `rolled_back` tells whether the design went back, not whether a rollback was asked
            // for (#3242): an app that says so is believed; an older one went back when it undid
            // every completed step (none completed: nothing to take back).
            let went_back = r.get("rolled_back").and_then(Value::as_bool).unwrap_or_else(|| r["undone"].as_u64() == Some(completed as u64));
            let rolled_back = rollback && went_back;
            let applied: Vec<usize> = if rolled_back { Vec::new() } else { (0..completed).collect() };
            (
                json!({
                    "ok": false,
                    "completed": r["completed"],
                    "failed": failed,
                    "rolled_back": rolled_back,
                    "undone": r["undone"],
                    "applied": applied,
                    // An app that refused the batch before its first step says what didn't run.
                    "not_run": r.get("not_run").filter(|v| v.is_array()).cloned().unwrap_or_else(|| json!((completed + 1..len).collect::<Vec<_>>())),
                    "results": results,
                }),
                true,
            )
        }
    };
    // The app timed the steps where they ran.
    if let Some(o) = reply.as_object_mut() {
        for k in ["timings", "total_ms"] {
            if let Some(v) = r.get(k).filter(|v| !v.is_null()) {
                o.insert(k.into(), v.clone());
            }
        }
    }
    (reply, error)
}

/// `measure {summary: true}`: an `inspect.measure` reply cut down to what checks a stage against
/// computed numbers: per body `name`, `volume_mm3`, `bbox`, `faces` and `closed` (a body that
/// could not be measured keeps its `error`), `body_count` and the total volume. An `items`
/// reply is small already and goes through as it is.
fn measure_summary(v: Value) -> Value {
    let Some(o) = v.as_object() else { return v };
    if !o.contains_key("bodies") {
        return v;
    }
    let bodies: Vec<Value> = o
        .get("bodies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|b| {
            let mut s = Map::new();
            let keys: &[&str] = if b.get("error").is_some() { &["name", "error"] } else { &["name", "volume_mm3", "bbox", "faces", "closed"] };
            for k in keys {
                s.insert((*k).to_string(), b.get(*k).cloned().unwrap_or(Value::Null));
            }
            Value::Object(s)
        })
        .collect();
    json!({
        "body_count": o.get("body_count").cloned().unwrap_or_else(|| json!(bodies.len())),
        "bodies": bodies,
        "total": {"volume_mm3": o.get("total").and_then(|t| t.get("volume_mm3")).cloned().unwrap_or(Value::Null)},
    })
}

/// A `[x, y, z]` JSON array as numbers.
fn xyz(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let p = [a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?];
    p.iter().all(|c| c.is_finite()).then_some(p)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// One cylinder as `body_topology {faces: "cylinders"}` reports it: the faces that lie on it,
/// merged by equal radius and the same axis line.
struct TopoCylinder {
    radius: f64,
    origin: [f64; 3],
    /// Unit length.
    axis: [f64; 3],
    /// Distances along `axis` from `origin` of the points found on the cylinder so far.
    extent: Option<(f64, f64)>,
    names: Vec<Value>,
}

/// How far a point lies from the cylinder's axis line, and how far along it from the origin.
fn axial(c: &TopoCylinder, p: [f64; 3]) -> (f64, f64) {
    let d = sub(p, c.origin);
    let along = dot(d, c.axis);
    let off = cross(d, c.axis);
    (dot(off, off).sqrt(), along)
}

/// Does `p` lie on the cylinder's surface, within `tol`?
fn on_cylinder(c: &TopoCylinder, p: [f64; 3], tol: f64) -> bool {
    (axial(c, p).0 - c.radius).abs() <= tol
}

fn widen(c: &mut TopoCylinder, p: [f64; 3]) {
    let t = axial(c, p).1;
    c.extent = Some(match c.extent {
        Some((lo, hi)) => (lo.min(t), hi.max(t)),
        None => (t, t),
    });
}

/// Positions and axes that agree within this are one cylinder (mm).
const CYLINDER_TOL: f64 = 1e-3;

/// `body_topology {faces: "cylinders"}`: the cylindrical faces of `model.faces`, the halves the
/// kernel splits one cylinder into merged (equal radius, the same axis line: origin on the axis,
/// direction up to sign), each with the extent along its axis that the body's own points on the
/// cylinder span (the faces' `point`s and the ends and midpoints of the edges lying on it). Built
/// from what `model.faces` and `model.edges` already reply, so it needs nothing of the kernel.
fn topology_cylinders(faces: &Value, edges: &Value) -> Value {
    let mut found: Vec<TopoCylinder> = Vec::new();
    for f in faces.as_array().into_iter().flatten() {
        let c = &f["cylinder"];
        let (Some(radius), Some(origin), Some(axis)) = (c["radius"].as_f64(), xyz(&c["origin"]), xyz(&c["axis"])) else { continue };
        let len = dot(axis, axis).sqrt();
        if !radius.is_finite() || radius <= 0.0 || !len.is_finite() || len < 1e-9 {
            continue;
        }
        let axis = [axis[0] / len, axis[1] / len, axis[2] / len];
        // The same cylinder: the same radius and axis direction (either way round), and this
        // face's origin on that axis line.
        let same = |k: &TopoCylinder| {
            let off = cross(sub(origin, k.origin), k.axis);
            (k.radius - radius).abs() <= CYLINDER_TOL && dot(k.axis, axis).abs() >= 1.0 - 1e-6 && dot(off, off).sqrt() <= CYLINDER_TOL
        };
        let name = f.get("name").cloned().unwrap_or(Value::Null);
        let at = match found.iter().position(same) {
            Some(i) => i,
            None => {
                found.push(TopoCylinder { radius, origin, axis, extent: None, names: Vec::new() });
                found.len() - 1
            }
        };
        if let Some(k) = found.get_mut(at) {
            k.names.push(name);
            if let Some(p) = xyz(&f["point"]) {
                widen(k, p);
            }
        }
    }
    // The ends of the cylinder: the vertices of the edges that lie on it (a circle's ends and
    // midpoint are on it; an edge of another face at most touches it in two points).
    for e in edges.as_array().into_iter().flatten() {
        let (Some(start), Some(mid), Some(end)) = (xyz(&e["start"]), xyz(&e["mid"]), xyz(&e["end"])) else { continue };
        for c in &mut found {
            let tol = CYLINDER_TOL.max(c.radius * 1e-4);
            if [start, mid, end].iter().all(|p| on_cylinder(c, *p, tol)) {
                widen(c, start);
                widen(c, end);
            }
        }
    }
    found
        .into_iter()
        .map(|c| {
            let mut v = json!({"radius": c.radius, "origin": c.origin, "axis": c.axis, "names": c.names});
            if let (Some((lo, hi)), Some(o)) = (c.extent, v.as_object_mut()) {
                o.insert("extent".into(), json!([lo, hi]));
            }
            v
        })
        .collect()
}

/// A command's result without what the design already had (`execute` or `batch` with `verbose: false`): a
/// feature's full body list (its created, changed and removed bodies stay), and the sketch ids
/// a sketch command repeats outside `created`.
fn compact(mut v: Value) -> Value {
    let Some(o) = v.as_object_mut() else { return v };
    if o.contains_key("created_bodies") {
        o.remove("bodies");
    }
    if let Some(created) = o.get("created").cloned() {
        for k in ["points", "curves", "constraints", "dimensions"] {
            if o.get(k).is_some_and(|x| created.get(k) == Some(x)) {
                o.remove(k);
            }
        }
    }
    v
}

/// `check_parameters`: try parameter values and put the design back (#3380). The checkpoint is
/// taken before anything changes; whatever happens after it, the design is restored, and a
/// restore that fails is an error saying the values are still in. Every agent session that
/// changed a parameter to see its effect spent three to five calls on this loop, and one saved
/// the design with the test values still in it.
fn check_parameters(b: &mut dyn Backend, a: &Map<String, Value>) -> Value {
    let (changes, reports) = match check_parameters_args(a) {
        Ok(x) => x,
        Err(e) => return text_result(&Value::String(e), true),
    };
    // A name the design has no parameter of is refused before the checkpoint, so nothing
    // changes: `parameters.change` would make one.
    if let Err(e) = known_parameters(b, &changes) {
        return text_result(&Value::String(e), true);
    }
    let Some(id) = exec(b, "edit.checkpoint", json!({})).ok().and_then(|v| v["checkpoint"].as_u64()) else {
        let e = "check_parameters: this app has no edit.checkpoint to put the design back with; use set_parameter and measure, and set the values back yourself";
        return text_result(&json!(e), true);
    };
    let outcome = try_parameters(b, &changes, &reports);
    let restore = exec(b, "edit.restore_checkpoint", json!({"checkpoint": id}));
    match (outcome, restore) {
        (Ok(mut reply), Ok(_)) => {
            reply["restored"] = json!(true);
            text_result(&reply, false)
        }
        (Err(e), Ok(_)) => text_result(&json!({"error": e, "restored": true, "note": "the design is back as it was"}), true),
        (outcome, Err(restore_error)) => {
            let mut reply = match outcome {
                Ok(r) => r,
                Err(e) => json!({"error": e}),
            };
            reply["restored"] = json!(false);
            reply["restore_error"] = json!(restore_error);
            reply["note"] = json!(
                "THE DESIGN STILL HOLDS THE TRIED VALUES: the checkpoint could not be restored. Set them back with set_parameter (or undo) before saving"
            );
            text_result(&reply, true)
        }
    }
}

/// `check_parameters`'s arguments, checked before anything is sent: the values to try (a
/// number or an expression each, as `set_parameter` takes its `value`) and what to report.
fn check_parameters_args(a: &Map<String, Value>) -> Result<(Map<String, Value>, Vec<&'static str>), String> {
    let set = a.get("set").and_then(Value::as_object).ok_or("check_parameters: `set` must be an object of parameter name: value")?;
    if set.is_empty() {
        return Err("check_parameters: `set` is empty; give at least one parameter name: value".into());
    }
    let mut changes = Map::new();
    for (name, v) in set {
        let name = name.trim();
        if name.is_empty() {
            return Err("check_parameters: a parameter name in `set` is empty".into());
        }
        if !matches!(v, Value::Number(_) | Value::String(_)) {
            let shown: String = name.chars().take(64).collect();
            return Err(format!("check_parameters: `set.{shown}` must be a number (mm) or an expression, as set_parameter's `value`"));
        }
        changes.insert(name.to_string(), v.clone());
    }
    let reports = match a.get("report").filter(|r| !r.is_null()) {
        None => REPORTS.to_vec(),
        Some(r) => {
            let items = r.as_array().ok_or("check_parameters: `report` must be an array of measure, cylinders, errors")?;
            let mut out: Vec<&'static str> = Vec::new();
            for item in items {
                let Some(known) = item.as_str().and_then(|s| REPORTS.iter().copied().find(|r| *r == s)) else {
                    return Err(format!("check_parameters: `report` items must be one of {}", REPORTS.join(", ")));
                };
                if !out.contains(&known) {
                    out.push(known);
                }
            }
            out
        }
    };
    Ok((changes, reports))
}

/// Refuse a name the design has no parameter of, before anything changes. An app without
/// `parameters.list` is not asked; the change itself then tells.
fn known_parameters(b: &mut dyn Backend, changes: &Map<String, Value>) -> Result<(), String> {
    let Ok(list) = exec(b, "parameters.list", json!({})) else { return Ok(()) };
    let Some(rows) = list["parameters"].as_array() else { return Ok(()) };
    let names: Vec<&str> = rows.iter().filter_map(|r| r["name"].as_str()).collect();
    let shown = |n: &str| format!("`{}`", n.chars().take(64).collect::<String>());
    let unknown: Vec<String> = changes.keys().filter(|n| !names.contains(&n.as_str())).map(|n| shown(n)).collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let user: Vec<String> = rows.iter().filter(|r| r["source"] == "user").filter_map(|r| r["name"].as_str()).take(40).map(shown).collect();
    let have = if user.is_empty() { "no user parameters".to_string() } else { format!("user parameters {}", user.join(", ")) };
    Err(format!("check_parameters: no parameter {}; the design has {have} (parameters.list shows every parameter)", unknown.join(", ")))
}

/// Apply the values and read back what was asked for: what each parameter is now, the
/// features rebuilt, and the reports.
fn try_parameters(b: &mut dyn Backend, changes: &Map<String, Value>, reports: &[&str]) -> Result<Value, String> {
    let (values, recomputed) = apply_parameters(b, changes)?;
    let mut reply = json!({"set": changes, "values": values, "recomputed": recomputed});
    // The bodies and each feature's state: one cheap read serves `errors` and `cylinders`.
    let design = if reports.iter().any(|r| *r != "measure") { Some(b.call("document.inspect", json!({"measure": false}))?) } else { None };
    for r in reports {
        match *r {
            "measure" => reply["measure"] = exec(b, "inspect.measure", json!({}))?,
            "errors" => {
                if let Some(d) = &design {
                    let pick = |key: &str| -> Value {
                        let rows = d["timeline"].as_array().into_iter().flatten().filter(|f| !f[key].is_null());
                        Value::Array(rows.map(|f| json!({"feature": f["id"], "name": f["name"], key: f[key]})).collect())
                    };
                    reply["errors"] = pick("error");
                    reply["warnings"] = pick("warning");
                    let params = d["params"].as_array().into_iter().flatten().filter(|p| !p["error"].is_null());
                    reply["parameter_errors"] = Value::Array(params.map(|p| json!({"name": p["name"], "error": p["error"]})).collect());
                }
            }
            "cylinders" => {
                if let Some(d) = &design {
                    let bodies: Vec<String> =
                        d["bodies"].as_array().into_iter().flatten().filter_map(|body| body["name"].as_str()).map(str::to_string).collect();
                    reply["cylinders"] = Value::Array(cylinders(b, &bodies));
                }
            }
            _ => {}
        }
    }
    Ok(reply)
}

/// Set the values with one `parameters.change {changes}`, so the design rebuilds once. An app
/// without `changes` (it asks for `name`, or refuses the key) gets one `parameters.change` per
/// name instead; any other refusal (an expression that does not evaluate) is the error. Returns
/// each parameter's value now and the features rebuilt.
fn apply_parameters(b: &mut dyn Backend, changes: &Map<String, Value>) -> Result<(Map<String, Value>, Value), String> {
    let combined = solidcraft_engine::batch_params::combined_params(changes);
    match exec(b, "parameters.change", combined) {
        Ok(v) => Ok((v["changed"].as_object().cloned().unwrap_or_default(), v["recomputed"].clone())),
        Err(e) if e.contains("`name`") || e.contains("`changes`") => {
            let mut values = Map::new();
            let mut recomputed = 0u64;
            for (name, expression) in changes {
                let v = exec(b, "parameters.change", json!({"name": name, "expression": expression}))?;
                values.insert(name.clone(), v["value"].clone());
                recomputed += v["recomputed"].as_u64().unwrap_or(0);
            }
            Ok((values, json!(recomputed)))
        }
        Err(e) => Err(e),
    }
}

/// Every cylindrical face of these bodies (`model.faces`), the halves one cylinder is split
/// into merged.
fn cylinders(b: &mut dyn Backend, bodies: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for body in bodies {
        match exec(b, "model.faces", json!({"body": body})) {
            Ok(f) => out.extend(merge_cylinders(body, f["faces"].as_array().map(Vec::as_slice).unwrap_or(&[]))),
            Err(e) => out.push(json!({"body": body, "error": e})),
        }
    }
    out
}

/// One cylinder of a body: the faces on it (one, or the halves a kernel splits it into).
struct Cylinder {
    radius: f64,
    origin: [f64; 3],
    axis: [f64; 3],
    faces: Vec<Value>,
    name: Value,
    area: f64,
}

fn vec3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    match (a.first()?.as_f64(), a.get(1)?.as_f64(), a.get(2)?.as_f64()) {
        (Some(x), Some(y), Some(z)) if x.is_finite() && y.is_finite() && z.is_finite() => Some([x, y, z]),
        _ => None,
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The cylindrical faces of one body as cylinders: faces with the same radius on the same
/// axis line are one cylinder, whatever point on the axis and whichever direction each half
/// reports. The origin is the axis point closest to the world origin and the axis points so
/// that its first non-zero component is positive, so the same cylinder reads the same whichever
/// kernel built it.
fn merge_cylinders(body: &str, faces: &[Value]) -> Vec<Value> {
    let mut found: Vec<Cylinder> = Vec::new();
    for f in faces {
        let c = &f["cylinder"];
        let (Some(radius), Some(origin), Some(axis)) = (c["radius"].as_f64(), vec3(&c["origin"]), vec3(&c["axis"])) else { continue };
        let len = dot(axis, axis).sqrt();
        if !(radius > 0.0) || !radius.is_finite() || !(len > 0.0) {
            continue;
        }
        let mut axis = axis.map(|x| x / len);
        if axis.iter().find(|x| x.abs() > 1e-9).is_some_and(|x| *x < 0.0) {
            // A zero component stays 0.0, not -0.0.
            axis = axis.map(|x| if x.abs() > 1e-9 { -x } else { 0.0 });
        }
        let t = dot(origin, axis);
        let origin = [origin[0] - t * axis[0], origin[1] - t * axis[1], origin[2] - t * axis[2]];
        let tol = 1e-4 * radius.max(1.0);
        let area = f["area"].as_f64().unwrap_or(0.0);
        let same = |k: &Cylinder| {
            let d = [k.origin[0] - origin[0], k.origin[1] - origin[1], k.origin[2] - origin[2]];
            (k.radius - radius).abs() <= tol && dot(k.axis, axis) >= 1.0 - 1e-8 && dot(d, d).sqrt() <= tol
        };
        match found.iter_mut().find(|k| same(k)) {
            Some(k) => {
                k.faces.push(f["index"].clone());
                k.area += area;
            }
            None => found.push(Cylinder { radius, origin, axis, faces: vec![f["index"].clone()], name: f["name"].clone(), area }),
        }
    }
    found
        .into_iter()
        .map(|k| json!({"body": body, "radius": k.radius, "origin": k.origin, "axis": k.axis, "faces": k.faces, "name": k.name, "area": k.area}))
        .collect()
}

static SHOT: AtomicU64 = AtomicU64::new(0);

/// `screenshot`: render or capture to a file, return it as an MCP image content block.
fn screenshot(b: &mut dyn Backend, a: &Map<String, Value>) -> Value {
    match capture(b, a) {
        Ok((png, info)) => json!({"content": [
            {"type": "image", "data": base64(&png), "mimeType": "image/png"},
            {"type": "text", "text": info.to_string()},
        ]}),
        Err(e) => text_result(&Value::String(e), true),
    }
}

fn capture(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<(Vec<u8>, Value), String> {
    // `grid` and `axes` shape a model render; the window shows what it shows.
    let guides: Vec<(&str, bool)> = ["grid", "axes"].into_iter().filter_map(|k| a.get(k).and_then(Value::as_bool).map(|v| (k, v))).collect();
    let window = match str_arg(a, "source") {
        Some("model") => false,
        Some("window") if !guides.is_empty() => {
            return Err("screenshot: `grid` and `axes` apply to model renders; use source \"model\" (the window shows what it shows)".into());
        }
        Some("window") => true,
        _ => b.has_ui() && guides.is_empty(),
    };
    let apply = |b: &mut dyn Backend, params: Value| b.call("engine.execute", json!({"command": "view.apply", "params": params}));
    match (str_arg(a, "view"), a.get("camera").filter(|c| !c.is_null())) {
        (Some(_), Some(_)) => return Err("screenshot: give `view` or `camera`, not both".into()),
        (None, Some(c)) => {
            apply(b, json!({"camera": c}))?;
        }
        (Some(v), None) if VIEWS.contains(&v) => {
            b.call("ui.view", json!({"view": v}))?;
            // Turning to a standard view also frames the model (like a axis view click).
            if v != "fit" && v != "home" {
                b.call("ui.view", json!({"view": "fit"}))?;
            }
        }
        // A saved named view.
        (Some(v), None) => {
            apply(b, json!({"view": v}))?;
        }
        (None, None) => {}
    }
    let keep = str_arg(a, "path").filter(|p| !p.trim().is_empty()).map(str::to_string);
    let path = keep.clone().unwrap_or_else(|| {
        let n = SHOT.fetch_add(1, Ordering::Relaxed);
        let dir = crate::scratch_dir().unwrap_or_else(vfs::temp_dir);
        dir.join(format!("shot-{n}.png")).to_string_lossy().to_string()
    });
    let size = |k: &str, d: u64| a.get(k).and_then(Value::as_u64).unwrap_or(d).clamp(16, 4096);
    let render = |b: &mut dyn Backend| {
        let mut params = json!({"path": path, "width": size("width", 960), "height": size("height", 600)});
        for (k, v) in &guides {
            params[*k] = json!(v);
        }
        b.call("ui.render", params)
    };
    let asked_window = str_arg(a, "source") == Some("window");
    let mut fallback = None;
    let (info, source) = if window {
        match b.call("ui.screenshot", json!({"path": path})) {
            Ok(v) => (v, "window"),
            // Asked for the window: say why there is none, so the caller can show it or ask for
            // a model render (#1677).
            Err(e) if asked_window => {
                return Err(format!("screenshot: the window gave no picture ({e}); show the window, or use source \"model\""));
            }
            // No presented frame (minimised or hidden window): a model render, and the reply says so.
            Err(e) => {
                fallback = Some(e);
                (render(b)?, "model")
            }
        }
    } else {
        (render(b)?, "model")
    };
    let read = vfs::read(&path, MAX_IMAGE_BYTES).map_err(|e| format!("{path}: {e} (is the app on another machine?)"));
    if keep.is_none() {
        let _ = vfs::remove(&path);
    }
    let png = read?;
    let (w, h) = match source {
        "model" => (json!(size("width", 960)), json!(size("height", 600))),
        _ => (info.get("width").cloned().unwrap_or(Value::Null), info.get("height").cloned().unwrap_or(Value::Null)),
    };
    let bytes = png.len();
    let mut reply = json!({"source": source, "path": keep, "width": w, "height": h, "bytes": bytes});
    if let Some(why) = fallback {
        reply["fallback"] = json!(format!("the window gave no picture ({why}); this is a model render"));
    }
    // What drew a model render (gpu, gpu-software or cpu), and the warning a CPU render carries.
    for k in ["renderer", "warning"] {
        if let Some(v) = info.get(k).filter(|v| !v.is_null()) {
            reply[k] = v.clone();
        }
    }
    Ok((png, reply))
}

/// Standard base64 with padding (RFC 4648).
pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
