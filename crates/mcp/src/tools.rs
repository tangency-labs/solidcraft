//! MCP tool definitions and dispatch. Every tool is a thin wrapper over engine commands
//! (`engine.execute`) and the inspection / rendering control methods.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value, json};
use solidcraft_engine::io::vfs;
use solidcraft_engine::timing;

use solidcraft_mcp_server::{Effect, object, string, tool};

use crate::backend::{Backend, STILL_RUNNING};
use crate::options::Options;

/// Most commands in one `batch` call.
const MAX_BATCH: usize = 10_000;

/// The largest script file `batch` reads (`path`).
const MAX_SCRIPT_BYTES: usize = 32 << 20;
/// Largest image returned by `screenshot`.
const MAX_IMAGE_BYTES: usize = 32 << 20;
/// Relative tolerance on an expected volume (`expect.tol`) when the call gives none.
const EXPECT_VOLUME_TOL: f64 = 0.002;
/// Absolute tolerance on each expected bounding-box coordinate, mm.
const EXPECT_BBOX_TOL_MM: f64 = 0.05;

const VIEWS: &[&str] = &["iso", "home", "front", "back", "top", "bottom", "left", "right", "fit"];
/// What `check_parameters` can report.
const REPORTS: &[&str] = &["measure", "cylinders", "errors"];

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

/// Every tool, with its JSON schema, as the server has them by default. The same list serves the
/// headless and the bridged backend.
pub fn tool_definitions() -> Vec<Value> {
    tool_definitions_with(Options::default())
}

/// Every tool, with its JSON schema, for these [`Options`] (`compact` changes `inspect_design`'s
/// arguments).
pub fn tool_definitions_with(opts: Options) -> Vec<Value> {
    vec![
        tool(
            "list_commands",
            "List commands",
            "The command catalog: id, label, toolbar tab/panel, shortcut, one-line parameter docs and whether the command is \
             enabled right now. Ids are dotted and lower-case (solid.extrude, sketch.create, \
             solid.fillet, sketch.inspect, timeline.rollback). Optional case-insensitive \
             substring `filter` over id, label, tab and panel (not the parameter docs, so a command id \
             finds that command rather than every command whose docs mention it). Narrow it further with `prefix` \
             (start of the id) and `panel` (a whole panel or tab name); shrink each entry with `compact` (id and label) \
             or `fields`; page it with `limit` and `offset`, which make the reply {commands, total, offset, next_offset}. \
             With none of these the reply is the whole catalog, about 350 KB.",
            object(
                json!({
                    "filter": string("Substring to match, e.g. \"sketch\", \"fillet\", \"pattern\""),
                    "prefix": string("Case-insensitive start of the command id, e.g. \"sketch.\""),
                    "panel": string("A whole toolbar panel or tab name, case-insensitive, e.g. \"Modify\" or \"SKETCH\""),
                    "compact": {"type": "boolean", "description": "Each entry is only {id, label} (default false)"},
                    "fields": {
                        "type": "array",
                        "items": {"type": "string", "enum": COMMAND_FIELDS},
                        "description": "Keep only these keys of each entry; overrides `compact`",
                    },
                    "limit": {"type": "integer", "minimum": 1, "description": "At most this many commands; the reply becomes {commands, total, offset, next_offset}"},
                    "offset": {"type": "integer", "minimum": 0, "description": "Skip this many matches first (default 0); with it the reply is paged as with `limit`"},
                }),
                &[],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "describe_command",
            "Describe a command",
            "One command's parameter docs and, for commands with a typed contract (every new command, and more of the older ones as they are converted), the \
             JSON Schemas (2020-12) of its parameters (`input`) and of a successful result (`output`); null for the others.",
            object(json!({"id": string("Command id, e.g. \"solid.extrude\"")}), &["id"]),
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
             When connected to the running SolidCraft app, view.pick {pixels: [[x,y]…]} answers the body, face, exact world point, normal and depth under pixels of a screenshot. \
             view.pick is unavailable in headless MCP, including with frame: \"model\". It is handled by the app and absent from list_commands and describe_command \
             (read-only; docs/control-protocol.md). \
             `expect` {volume_mm3, tol, bodies, bbox} checks the design after the command (as `batch` does) and flags a mismatch.",
            object(
                json!({
                    "command": string("Command id (see list_commands)"),
                    "params": {"type": "object", "description": "The command's parameters"},
                    "verbose": {"type": "boolean", "description": "false: omit the full feature body list and sketch ids repeated outside `created`; keep created_bodies, changed_bodies and removed_bodies. When a sketch command creates more points than its new curves have ends (spline poles), `created.points` becomes `created.point_count`. A `created` list of more than 20 ids (a drawing of many curves) is left out and counted instead: `curve_count`, `constraint_count`, `dimension_count` (and `point_count`); `sketch.inspect` lists every id (default true)"},
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
             list_commands (engine.commands, narrowed by its arguments), describe_command (engine.describe), execute ({command, params}), \
             measure (inspect.measure), compare_picture (inspect.compare_picture, numbers only: no overlay PNG), \
             inspect_design (document.inspect), undo, redo, export, save, open, new_design, crash_reports; screenshot, \
             body_topology, batch and pending can't be steps. On an error the steps already run are undone (unless \
             rollback: false), so the batch is all-or-nothing; the reply names the failing step (index from 0), its error, \
             every result before it, whether the design really went back (rolled_back), the steps whose changes are in the \
             design (applied) and those never sent (not_run). The usual way to build a part: sketch, draw, dimension, \
             finish, extrude, fillet… in one call. A long one (a polyline of hundreds of points) can be written to a JSON \
             file and passed as `path` instead of `commands`. Give `expect` {volume_mm3, tol, bodies, bbox} with the numbers \
             you computed beforehand: after the last step the design is measured and compared, so no separate measure call is \
             needed; a mismatch is flagged as an error naming it, and the design is kept as built.",
            object(
                json!({
                    "commands": {
                        "type": "array",
                        "description": "Steps: [{\"command\": id, \"params\": {...}}, ...]",
                        "items": {"type": "object", "properties": {"command": {"type": "string"}, "params": {"type": "object"}}, "required": ["command"]},
                    },
                    "path": string("Instead of `commands`: a JSON file holding the steps, an array or {\"commands\": [...]} (as \
                     `solidcraft-cli run` takes and `solidcraft-cli script` writes), read by this server within the folders the \
                     agent may use; the steps run as if they had been sent here"),
                    "rollback": {"type": "boolean", "description": "Undo completed steps when one fails (default true)"},
                    "coalesce_parameters": {"type": "boolean", "description": "Set each run of consecutive plain parameter edits (parameters.change or set_parameter with a name and a value, or changes) together, rebuilding the design once for the run instead of once per edit; a run that is refused goes step by step, naming the refused step. Other steps still see the design up to date. Each step keeps its own result; the run is one undo step (default false)"},
                    "timings": {"type": "boolean", "description": "Add `timings`: [{index, command, ms}] for every step run (the failing one included) and `total_ms` (default false)"},
                    "profile": {"type": "boolean", "description": "Like timings, and each step's `profile`: {boolean_ms, tessellate_ms, other_ms}, its time in kernel booleans, in tessellation and the rest (default false)"},
                    "verbose": {"type": "boolean", "description": "false: a compact reply. Feature steps leave out the full body list (created_bodies, changed_bodies and removed_bodies stay), sketch steps the ids they repeat outside `created`, and `created.points` becomes `created.point_count` when there are more points than the new curves have ends; a `created` list of more than 20 ids is left out and counted instead (`curve_count` and the like); `sketch.inspect` lists every id (default true)"},
                    "expect": expect_schema(),
                }),
                &[],
            ),
            Effect::Files,
        ),
        inspect_design_tool(opts),
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
            object(
                json!({
                    "body": string("Body name (default: all bodies)"),
                    "names_only": {"type": "boolean", "description": "Only each body's name (assembly occurrences included) and its definition's, without measuring (default false)"},
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
            object(
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
            object(
                json!({
                    "name": string("Parameter name"),
                    "value": {"anyOf": [{"type": "number"}, {"type": "string"}], "description": "Number or expression (or give it as `expression`)"},
                    "expression": {"anyOf": [{"type": "number"}, {"type": "string"}], "description": "Same as `value` (the parameters.change name); give one of the two"},
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
             measure, body_topology, set_parameter back. On a large design keep the reply small with `bodies` (only those \
             bodies are measured and their cylinders listed) and `summary: true` (measure as the measure tool's summary).",
            object(
                json!({
                    "set": {
                        "type": "object",
                        "description": "Values to try, by parameter name: a number (mm) or an expression (\"140 mm\", \"wall * 2\"), as set_parameter's `value`",
                        "additionalProperties": {"anyOf": [{"type": "number"}, {"type": "string"}]},
                    },
                    "report": {
                        "type": "array",
                        "items": {"type": "string", "enum": REPORTS},
                        "description": "What to report: measure, cylinders, errors (default all three)",
                    },
                    "bodies": {"type": "array", "items": {"type": "string"}, "description": "Only these bodies, by name, for measure and cylinders (default: all)"},
                    "summary": {"type": "boolean", "description": "measure as the measure tool's `summary: true`: per body only name, volume_mm3, bbox {min, max}, faces and closed, and total.volume_mm3 (default false: the full reply)"},
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
            object(
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
            "compare_picture",
            "Compare with a reference picture",
            "Compare the model's outline with the part in a reference picture — a concept render, a product photo, one view off a design board. \
             The model's outline is `inspect.silhouette` from `view` (default front), `camera` and `bodies`; the picture's part is found by growing \
             the background in from the border, so a backdrop's gradual shading stays background. The compare is outline only: holes are not \
             compared — both the picture's and the model's holes are filled. `reference` is a PNG or JPEG inside the server's \
             folders. `crop_px` / `crop_fraction` take one view off a board: [x, y, w, h] in pixels or as fractions 0..1 of the picture; \
             `part_fills_crop: true` says the crop box is the part itself, for a crop that tightly frames it. `fit` names \
             the dimension the scale comes from (\"width\", \"height\", \"both\"; default \"width\") — one uniform scale, never stretched per axis; \
             \"both\" scales by width and reports `aspect_mismatch` when the shapes differ. Returns as text the overlap (0..1) and the model's top, \
             bottom, left and right edges against the picture's in mm at nine stations (positive where the model reaches past the picture), and as \
             an image the overlay PNG: model red, picture blue.",
            object(
                json!({
                    "reference": string("PNG or JPEG to match, inside the server's folders"),
                    "view": string("Standard view (front, top, right, iso, ...) the picture is seen from (default front)"),
                    "camera": {"type": "object", "description": "Look from this camera instead: {target: [x,y,z], yaw, pitch, distance, fov?} (radians, mm; as view.apply). A photo taken in perspective gets an fov"},
                    "bodies": {"type": "array", "items": {"type": "string"}, "description": "Body names to outline (default: the visible bodies)"},
                    "crop_px": {"type": "array", "items": {"type": "number"}, "description": "[x, y, w, h] in pixels"},
                    "crop_fraction": {"type": "array", "items": {"type": "number"}, "description": "[x, y, w, h] as fractions 0..1 of the picture"},
                    "fit": {"type": "string", "enum": ["width", "height", "both"], "description": "The dimension the one uniform scale comes from (default width); both scales by width and reports aspect_mismatch"},
                    "rows": {"type": "integer", "description": "The model outline's height in pixels (default 240)"},
                }),
                &["reference"],
            ),
            Effect::Files,
        ),
        tool(
            "export",
            "Export",
            "Export bodies to STEP (.step/.stp), IGES (.iges/.igs), 3MF (.3mf), binary STL (.stl), ASCII STL (format stla), OBJ or GLB (.glb: binary glTF \
             in metres with Y up, with each body's colours, metallic and roughness, and each occurrence a named node holding its part's bodies; \
             transparent bodies are refused). The format \
             comes from the extension unless given. URDF (.urdf) exports the whole assembly for robot simulators: a link per \
             component placement with mass and inertia, joints from the assembly's joints, and STL meshes in metres written \
             next to it (docs/urdf.md); RViz needs mesh_prefix such as package://robot/meshes/.",
            object(
                json!({
                    "path": string("Output file"),
                    "format": {"type": "string", "enum": ["step", "stp", "iges", "igs", "3mf", "3mf-tagged", "stl", "stla", "obj", "glb", "urdf"]},
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
            object(
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
            object(json!({}), &[]),
            Effect::ReadOnly,
        ),
        tool(
            "pending",
            "Result of a running request",
            "Bridged to the app: the result of a request that was still running when its tool call returned (an error \
             starting \"still running:\"). Waits for it up to 60 s: then its own result (or error), or done: false while \
             it still runs (call again). pending: null when nothing is running. A request still running was not \
             cancelled (`cancel` cancels it): don't send it again; other calls wait for it before they are sent.",
            object(json!({}), &[]),
            Effect::ReadOnly,
        ),
        tool(
            "cancel",
            "Cancel",
            "Bridged: cancel the request that was still running in the app when its call returned (an error starting \
             \"still running:\"), without waiting for it. One still waiting its turn will not run; a design change or \
             export computing off the app's window thread is stopped (Preferences > AI): the design stays as it was, \
             though a stopped export may still write its file. One already applying on the window thread can't be \
             cancelled: the error says so, and it will still finish. Only \
             this server's own request can be cancelled. Headless calls always run to their end, so there is nothing to \
             cancel.",
            object(json!({}), &[]),
            Effect::Edits,
        ),
        tool("undo", "Undo", "Undo the last change to the design.", object(json!({}), &[]), Effect::Edits),
        tool("redo", "Redo", "Redo the last undone change.", object(json!({}), &[]), Effect::Edits),
        tool(
            "new_design",
            "New design",
            "Start a new, empty design (unsaved changes to the current one are discarded).",
            object(json!({"name": string("Design name (default Untitled)")}), &[]),
            Effect::Replaces,
        ),
        tool(
            "open",
            "Open a design",
            "Open a .sc design file (or a .solidcraft one from an earlier release), a STEP file (.step/.stp: a new design whose bodies come from the file, as an \
             Import base feature in a component named after the file; an assembly's parts become nested components) or a \
             mesh (.3mf/.stl/.obj/.glb/.gltf: mesh bodies in such a component; a glTF file's nodes become nested components and its \
             materials appearances); the result reports the component (id, component_name). Replaces the current design; insert into it \
             instead with the file.insert_step (STEP) or file.insert_mesh (mesh) commands. A STEP file from a system \
             that has Y up opens lying on its back: `up: \"y\"` turns its component a quarter turn about X (Y becomes Z). glTF is Y up, \
             so it is turned by default (`up: \"z\"` keeps its axes). `names` picks what the bodies are named after when the solids' own names say nothing useful.",
            object(
                json!({
                    "path": string("Path to a .sc (or .solidcraft), .step/.stp, .3mf, .stl, .obj, .glb or .gltf file"),
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
            object(
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
            object(
                json!({
                    "skill": string("Skill name, such as solidcraft-modeling"),
                    "file": {"type": "string", "description": "Document of that skill", "default": "reference.md"},
                }),
                &["skill"],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "cad_status",
            "CAD status and environment health",
            "Fast inspection of session state and environment without the overhead of body measurement: backend mode              (headless vs remote bridged app), open design name, units, active sketch (if editing a sketch),              feature count, body count, sketch count, parameter count, undo/redo depth, and timeline health.",
            object(json!({}), &[]),
            Effect::ReadOnly,
        ),
        tool(
            "artifacts",
            "List generated CAD artifacts",
            "Scan the workspace and allowed folders for generated CAD models, drawings and exports              (.solidcraft, .step, .stp, .dxf, .stl, .3mf, .obj, .urdf, .png). Returns file paths,              file sizes in bytes, formats, and modification timestamps. Optional `filter` by file extension or name substring.",
            object(
                json!({
                    "filter": string("Optional file name or extension substring, e.g. \".dxf\", \"bracket\", \".step\""),
                    "dir": string("Subdirectory or specific folder to scan (default: allowed paths / design folder)"),
                }),
                &[],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "materials",
            "Inspect and assign materials and appearances",
            "List available physical materials (density, mass properties) and visual appearances (color, opacity),              or assign them to bodies and faces. Actions: `list` (default: lists built-in and user materials, and appearances),              or `assign` (assigns a `material` or `appearance` / `color` to `bodies` or `faces`).",
            object(
                json!({
                    "action": {"type": "string", "enum": ["list", "assign"], "description": "Action to perform (default: list)"},
                    "bodies": {"type": "array", "items": {"type": "string"}, "description": "Body names to assign to"},
                    "faces": {"type": "array", "items": {"type": "object"}, "description": "Face selections to assign appearance to: [{\"body\": \"Body1\", \"point\": [x,y,z]}] or [[x,y,z]]"},
                    "material": string("Physical material name to assign (e.g. \"Steel\", \"Aluminum\", \"ABS\")"),
                    "appearance": string("Appearance name to assign from library or custom"),
                    "color": {"description": "Color as \"#rrggbb\" hex or [r, g, b] array 0..255"},
                    "opacity": {"type": "number", "description": "Opacity 0.0 to 1.0 (default 1.0)"},
                }),
                &[],
            ),
            Effect::Edits,
        ),
        tool(
            "find_faces",
            "Find faces or edges by their geometry",
            "Select a body's faces or edges by what they are instead of by index: planar faces facing a direction,              cylinders of a radius or axis, straight edges along a direction, the edges of one face, whatever lies              furthest along a direction (`extreme`: the top face, the bottom edges) or nearest a point. Each match's              `pick` is the [x, y, z] point the commands take (a face's point lies on it; an edge's is its midpoint, as              `solid.fillet` and `solid.chamfer` take it), with its `name` for the face commands. `distinct_cylinders`              counts cylindrical matches by axis and radius (a hole wall may be split into two faces).",
            object(query_schema(), &["body"]),
            Effect::ReadOnly,
        ),
        tool(
            "verify_design",
            "Verify the design against expectations",
            "Check many expectations in one call and get each one's verdict, measured value and expected value:              `{\"check\": \"volume\" | \"mass\", \"body\"?, \"equals\"? (with \"tol_rel\"?, default 1e-3), \"min\"?, \"max\"?}`,              `{\"check\": \"body_count\", \"equals\"?, \"min\"?, \"max\"?}`, `{\"check\": \"bbox\", \"body\"?, \"size\"?: [x,y,z]              (\"tol\"? mm, default 0.01), \"fits\"?: [x,y,z]}`, `{\"check\": \"timeline_ok\"}` (no feature fails),              `{\"check\": \"clearance\", \"a\", \"b\", \"min_mm\"}`, `{\"check\": \"no_interference\", \"bodies\"?}` and              `{\"check\": \"count\", \"query\": {find_faces arguments}, \"of\"?: \"matches\" | \"cylinders\", \"equals\"?, \"min\"?, \"max\"?}`.              A check that cannot be measured fails with its error; the others still run.",
            object(
                json!({
                    "checks": {"type": "array", "items": {"type": "object"}, "description": "The expectations (at most 100), as described above"},
                }),
                &["checks"],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "dfm_clearance_check",
            "DFM Clearance Check for 3D Print / CNC",
            "Measure the smallest gap between two bodies, compare it with a safe clearance (default 0.25 mm for FDM),              report a collision and say how much to widen the gap.",
            object(
                json!({
                    "body_a": string("First body name"),
                    "body_b": string("Second body name"),
                    "min_clearance_mm": {"type": "number", "description": "Smallest safe gap (mm, default 0.25 for FDM 3D printing)"},
                }),
                &["body_a", "body_b"],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "fdm_print_estimate",
            "FDM Print Estimate (filament and build volume)",
            "Estimate the plastic an FDM print of bodies takes (a shell of `wall_mm` plus `infill_percent` of the inside;              supports, brims and top/bottom layers are not counted, so a slicer's figure differs) as volume, grams and              metres of filament, and check that each body fits the printer's build volume as placed or turned by 90°.",
            object(
                json!({
                    "bodies": {"type": "array", "items": {"type": "string"}, "description": "Body names (default: all bodies)"},
                    "material": {"type": "string", "enum": ["PLA", "PETG", "ABS", "ASA", "TPU", "PA"], "description": "Filament (default PLA)"},
                    "density_g_cm3": {"type": "number", "description": "Filament density instead of `material`"},
                    "infill_percent": {"type": "number", "description": "Infill density, 0 to 100 (default 15)"},
                    "wall_mm": {"type": "number", "description": "Shell thickness (default 1.2: three 0.4 mm perimeters)"},
                    "filament_diameter_mm": {"type": "number", "description": "Filament diameter (default 1.75)"},
                    "build_volume_mm": {"type": "array", "items": {"type": "number"}, "description": "Printer build volume [x, y, z] (default [220, 220, 250])"},
                }),
                &[],
            ),
            Effect::ReadOnly,
        ),
        tool(
            "export_views",
            "Export standard drawing views as DXF",
            "Write orthographic (parallel) views of the shown bodies as one 2D DXF each, in mm, with hidden-line removal:              visible edges on layer VISIBLE and, with `hidden: dashed` (the default), hidden edges on layer HIDDEN. Views              are the app's standard views (front, back, left, right, top, bottom, iso), framed on the bodies' bounding              box. Files are `<path stem>_<view>.dxf`. Curves are approximated within `tolerance`; there is no sheet,              title block or dimensions yet: each view's `size_mm` (the bounding box seen from it) helps lay them out.",
            object(
                json!({
                    "path": string("Base path, e.g. \"out/bracket.dxf\" writes out/bracket_front.dxf, out/bracket_top.dxf, …"),
                    "views": {"type": "array", "items": {"type": "string", "enum": ["front", "back", "left", "right", "top", "bottom", "iso"]}, "description": "Views to write (default front, top, right, iso)"},
                    "hidden": {"type": "string", "enum": ["dashed", "omit"], "description": "Hidden edges on layer HIDDEN, or left out (default dashed)"},
                    "tolerance": {"type": "number", "description": "Curve chord tolerance in mm, 0.0001 to 1 (default 0.01)"},
                }),
                &["path"],
            ),
            Effect::Files,
        ),
    ]
}

/// `inspect_design`'s definition: as it always was, or with compact output its sketch summaries,
/// `sketches` by name and the `bodies` filter.
fn inspect_design_tool(opts: Options) -> Value {
    if !opts.compact {
        return tool(
            "inspect_design",
            "Inspect the design",
            "The whole design: user parameters (expression, value, errors), the timeline (features with type, suppressed, \
         rolled back, error, warning, timing), bodies (volume, area, centre of mass, bounding box, face/edge/vertex \
         counts), sketches (curves with ids and coordinates, points, constraints, profiles, degrees of freedom, solve \
         status), assembly joints (what each origin is attached to: occurrence, the feature, face, edge or sketch \
         curve its face names refer to, whether it resolves and why not; `execute` `joint.inspect {joint}` gives one), \
         the active sketch, selection and undo depth. On a large design keep the reply small with `timeline: \"summary\"` \
         (each feature as id, name, type, and error or warning only when it has one) or false, and `bodies: \"names\"` or false.",
            object(
                json!({
                    "measure": {"type": "boolean", "description": "Measure bodies (default true)"},
                    "sketches": {"type": "boolean", "description": "Full sketch contents (default true; false = summary only)"},
                    "joints": {"type": "boolean", "description": "Joint definitions as joint.inspect gives them, when the design has joints (default true)"},
                    "timeline": timeline_arg(),
                    "bodies": {"anyOf": [{"type": "boolean"}, {"type": "string"}], "description": BODIES_ARG},
                }),
                &[],
            ),
            Effect::ReadOnly,
        );
    }
    tool(
        "inspect_design",
        "Inspect the design",
        "The whole design: user parameters (expression, value, errors), the timeline (features with type, suppressed, \
         rolled back, error, warning, timing), bodies (volume, area, centre of mass, bounding box, face/edge/vertex \
         counts), sketches, the active sketch, selection and undo depth. Sketches are summarised by default (id, name, \
         plane, counts of points, curves, constraints and profiles, degrees of freedom, solve status); their full contents \
         (curves with ids and coordinates, points, constraints, profiles) only for the sketches named in `sketches`, or \
         all with `sketches: true` (large: a 720-point sketch is some 200 KB). Assembly joints: what each origin is \
         attached to (`execute` `joint.inspect {joint}` gives one). On a large design keep the reply small with \
         `timeline: \"summary\"` (each feature as id, name, type, and error or warning only when it has one) or false, and \
         `bodies: \"names\"` or false. Arguments: measure, sketches, bodies, timeline, joints.",
        object(
            json!({
                "measure": {"type": "boolean", "description": "Measure bodies (default true)"},
                "sketches": {
                    "anyOf": [
                        {"type": "boolean"},
                        {"type": "string"},
                        {"type": "array", "items": {"anyOf": [{"type": "string"}, {"type": "integer"}]}},
                    ],
                    "description": "Which sketches in full: a list of sketch names or ids, true (all of them), or false / \"summary\" (none: summaries only, the default)",
                },
                "bodies": {
                    "anyOf": [{"type": "array", "items": {"type": "string"}}, {"type": "boolean"}, {"type": "string"}],
                    "description": "Only these bodies, by name (default: all); or \"names\" (only the body names, nothing measured) or false (no bodies)",
                },
                "joints": {"type": "boolean", "description": "Joint definitions as joint.inspect gives them, when the design has joints (default true)"},
                "timeline": timeline_arg(),
            }),
            &[],
        ),
        Effect::ReadOnly,
    )
}

/// `inspect_design`'s `timeline` argument, as `document.inspect` takes it. No `enum`: Moonshot
/// refuses every tool when an argument of two types has one (#15306), so [`inspect_words`] checks
/// the word. The two types are an `anyOf`, not a list of types, which Gemini refuses (#16257).
fn timeline_arg() -> Value {
    json!({
        "anyOf": [{"type": "boolean"}, {"type": "string"}],
        "description": "true (default): every feature with its state and timing; \"summary\": each feature as id, name and type, with error, warning, suppressed and rolled_back only when set; false: no timeline",
    })
}

/// `inspect_design`'s words, checked before anything is sent: `timeline` takes "summary", and
/// `bodies` (when `bodies_word`) "names". Their schemas list no values (see [`timeline_arg`]).
fn inspect_words(a: &Map<String, Value>, bodies_word: bool) -> Result<(), String> {
    if a.get("timeline").and_then(Value::as_str).is_some_and(|w| w != "summary") {
        return Err("inspect_design: `timeline` must be true, false or \"summary\"".into());
    }
    if bodies_word && a.get("bodies").and_then(Value::as_str).is_some_and(|w| w != "names") {
        return Err("inspect_design: `bodies` must be true, false or \"names\"".into());
    }
    Ok(())
}

/// `inspect_design`'s `bodies` argument without compact output.
const BODIES_ARG: &str = "true (default): every body as `measure` says; \"names\": only the body names, nothing measured; false: no bodies";

/// A tool result: content blocks plus the error flag. JSON is pretty-printed, or compact with
/// compact output.
fn text_result(v: &Value, is_error: bool, opts: Options) -> Value {
    let text = match v {
        Value::String(s) => s.clone(),
        v => opts.json_text(v),
    };
    let mut r = solidcraft_mcp_server::text_result(text, is_error);
    if is_error
        && let Some(reason) = v
            .as_str()
            .and_then(solidcraft_engine::panels::commands::structured_error)
            .or_else(|| v.get("failed").and_then(|failed| failed.get("panel_error")).cloned())
    {
        r["structuredContent"] = json!({"panel_error":reason});
    }
    r
}

/// Run a tool, as the server does by default. Failures are tool results with `isError: true`
/// (the model sees the message); nothing here panics on hostile arguments.
pub fn call_tool(b: &mut dyn Backend, name: &str, args: &Value) -> Value {
    call_tool_with(b, name, args, Options::default())
}

/// [`call_tool`] with these [`Options`].
pub fn call_tool_with(b: &mut dyn Backend, name: &str, args: &Value, opts: Options) -> Value {
    let empty = Map::new();
    let a = args.as_object().unwrap_or(&empty);
    let r = match name {
        "screenshot" => return screenshot(b, a),
        "compare_picture" => return compare_picture(b, a, opts),
        "batch" => return batch(b, a, opts),
        "check_parameters" => return check_parameters(b, a, opts),
        "execute" => return execute(b, a, opts),
        "inspect_design" if opts.compact => inspect_design_compact(b, a),
        _ => dispatch(b, name, a),
    };
    match r {
        Ok(v) => text_result(&v, false, opts),
        Err(e) => text_result(&Value::String(e), true, opts),
    }
}

/// `execute`: one command, then its `expect` checked against the design.
fn execute(b: &mut dyn Backend, a: &Map<String, Value>, opts: Options) -> Value {
    let expect = match Expect::parse(a) {
        Ok(e) => e,
        Err(e) => return text_result(&Value::String(e), true, opts),
    };
    match dispatch(b, "execute", a) {
        Ok(v) => with_expectation(b, v, expect, opts),
        Err(e) => text_result(&Value::String(e), true, opts),
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
fn with_expectation(b: &mut dyn Backend, mut reply: Value, expect: Option<Expect>, opts: Options) -> Value {
    let Some(expect) = expect else { return text_result(&reply, false, opts) };
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
    let Some(note) = note else { return text_result(&reply, false, opts) };
    let mut r = text_result(&reply, true, opts);
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
    // An id that is exactly a registered tool name confuses the two namespaces: name the
    // distinction and its recovery instead of offering unrelated closest-command suggestions.
    if id == command.trim() && is_tool_name(id) {
        return tool_name_error(id);
    }
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

/// Whether `name` is exactly a registered MCP tool name, read from the tool definitions
/// themselves so the two namespaces can never drift apart.
fn is_tool_name(name: &str) -> bool {
    tool_definitions().iter().any(|t| t["name"].as_str() == Some(name))
}

/// The reply when a rejected command id is exactly a tool name: the recovery belongs to the
/// tool namespace, not to a typo's closest-command suggestions.
fn tool_name_error(id: &str) -> String {
    format!(
        "unknown command `{id}`: `{id}` is an MCP tool, not an engine command id. Use MCP tools/list for its tool schema, or list_commands for engine command ids."
    )
}

/// A `describe_command` rejection of an id that is exactly a tool name gets the namespace
/// hint; every other error is left exactly as the engine said it.
fn describe_error(id: &str, error: String) -> String {
    if error.starts_with(&format!("unknown command `{id}`")) && is_tool_name(id) {
        return tool_name_error(id);
    }
    error
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

/// `measure`'s arguments (without `items`) as `inspect.measure` takes them.
fn measure_params(a: &Map<String, Value>) -> Value {
    let mut p = pick(a, &["names_only"]);
    if let Some(body) = str_arg(a, "body") {
        p["bodies"] = json!([body]);
    }
    p
}

/// `inspect_design`'s arguments as `document.inspect` takes them: `measure`, and `keys` only when
/// given, so an app from before them is asked as it always was.
fn inspect_params(a: &Map<String, Value>, measure: bool, keys: &[&str]) -> Value {
    let mut p = pick(a, keys);
    p["measure"] = json!(measure);
    p
}

/// `summary` narrows measured bodies; a `names_only` list has nothing to narrow.
fn measure_summary_wanted(a: &Map<String, Value>) -> bool {
    let flag = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
    flag("summary") && !flag("names_only")
}

fn dispatch(b: &mut dyn Backend, name: &str, a: &Map<String, Value>) -> Result<Value, String> {
    match name {
        "list_commands" => {
            let query = CommandQuery::parse(a)?;
            let all = b.call("engine.commands", json!({}))?;
            Ok(query.apply(all))
        }
        "describe_command" => {
            let id = str_arg(a, "id").ok_or("`id` is required")?;
            // The engine rejects the requested id inside its own error, so the tool-name
            // hint must inspect the `id`, not the outer `engine.describe` command `exec` sees.
            exec(b, "engine.describe", json!({"command": id})).map_err(|e| describe_error(id, e))
        }
        "execute" => {
            let id = str_arg(a, "command").ok_or("`command` is required")?;
            let v = exec(b, id, a.get("params").cloned().filter(Value::is_object).unwrap_or(json!({})))?;
            Ok(if a.get("verbose").and_then(Value::as_bool).unwrap_or(true) { v } else { compact(v) })
        }
        "inspect_design" => {
            inspect_words(a, true)?;
            let measure = a.get("measure").and_then(Value::as_bool).unwrap_or(true);
            let mut d = b.call("document.inspect", inspect_params(a, measure, &["timeline", "bodies"]))?;
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
            let m = exec(b, "inspect.measure", measure_params(a))?;
            Ok(if measure_summary_wanted(a) { measure_summary(m) } else { m })
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
        "cancel" => b.call("control.cancel", json!({})),
        "undo" => exec(b, "edit.undo", json!({})),
        "redo" => exec(b, "edit.redo", json!({})),
        "new_design" => exec(b, "file.new", pick(a, &["name"])),
        "open" => exec(b, "doc.open", pick(a, &["path", "up", "names"])),
        "save" => exec(b, "file.save", pick(a, &["path", "format"])),
        "cad_status" => cad_status(b, a),
        "artifacts" => artifacts(b, a),
        "materials" => materials(b, a),
        "find_faces" => find_faces(b, a),
        "verify_design" => verify_design(b, a),
        "dfm_clearance_check" => dfm_clearance_check(b, a),
        "fdm_print_estimate" => fdm_print_estimate(b, a),
        "export_views" => export_views(b, a),
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// The command catalog filtered by a case-insensitive substring of id, label, panel or tab
/// (`list_commands`; not the parameter docs, which name other commands, #269).
/// The keys of a `list_commands` entry, which `fields` may keep.
const COMMAND_FIELDS: &[&str] = &["id", "label", "tab", "panel", "icon", "shortcut", "params", "enabled", "disabled_reason", "planned"];

/// How `list_commands` narrows the catalog (#11726): which commands, which keys of each, and
/// which page. The default keeps the whole catalog as it is.
#[derive(Clone, Debug, Default, PartialEq)]
struct CommandQuery {
    filter: Option<String>,
    prefix: Option<String>,
    panel: Option<String>,
    fields: Option<Vec<String>>,
    limit: Option<usize>,
    offset: Option<usize>,
}

impl CommandQuery {
    fn parse(a: &Map<String, Value>) -> Result<Self, String> {
        let text = |k: &str| -> Result<Option<String>, String> {
            match a.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.to_lowercase()).filter(|s| !s.is_empty())),
                Some(_) => Err(format!("`{k}` must be a string")),
            }
        };
        let count = |k: &str, least: u64| -> Result<Option<usize>, String> {
            match a.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_u64()
                    .filter(|n| *n >= least)
                    .map(|n| Some(usize::try_from(n).unwrap_or(usize::MAX)))
                    .ok_or_else(|| format!("`{k}` must be a whole number of at least {least}")),
            }
        };
        let compact = match a.get("compact") {
            None | Some(Value::Null) => false,
            Some(v) => v.as_bool().ok_or("`compact` must be true or false")?,
        };
        let fields = match a.get("fields") {
            None | Some(Value::Null) => compact.then(|| vec!["id".to_string(), "label".to_string()]),
            Some(Value::Array(v)) => {
                let mut keep = Vec::new();
                for f in v {
                    let name = f.as_str().filter(|n| COMMAND_FIELDS.contains(n)).ok_or_else(|| {
                        let shown: String = f.to_string().chars().take(64).collect();
                        format!("`fields` takes the entry keys {}; not {shown}", COMMAND_FIELDS.join(", "))
                    })?;
                    if !keep.iter().any(|k| k == name) {
                        keep.push(name.to_string());
                    }
                }
                Some(keep)
            }
            Some(_) => return Err("`fields` must be a list of entry keys".into()),
        };
        Ok(Self {
            filter: text("filter")?,
            prefix: text("prefix")?,
            panel: text("panel")?,
            fields,
            limit: count("limit", 1)?,
            offset: count("offset", 0)?,
        })
    }

    fn matches(&self, c: &Value) -> bool {
        let field = |k: &str| c.get(k).and_then(Value::as_str).map(str::to_lowercase).unwrap_or_default();
        self.filter.as_ref().is_none_or(|f| ["id", "label", "panel", "tab"].iter().any(|k| field(k).contains(f.as_str())))
            && self.prefix.as_ref().is_none_or(|p| field("id").starts_with(p.as_str()))
            && self.panel.as_ref().is_none_or(|p| field("panel") == *p || field("tab") == *p)
    }

    /// The catalog narrowed: a list, or `{commands, total, offset, next_offset}` when paged.
    fn apply(&self, all: Value) -> Value {
        let Value::Array(all) = all else { return all };
        let hits: Vec<Value> = all.into_iter().filter(|c| self.matches(c)).collect();
        let total = hits.len();
        let offset = self.offset.unwrap_or(0).min(total);
        let take = self.limit.unwrap_or(usize::MAX);
        let page: Vec<Value> = hits
            .into_iter()
            .skip(offset)
            .take(take)
            .map(|c| match (&self.fields, c) {
                (Some(keep), Value::Object(m)) => Value::Object(m.into_iter().filter(|(k, _)| keep.contains(k)).collect()),
                (_, c) => c,
            })
            .collect();
        if self.limit.is_none() && self.offset.is_none() {
            return Value::Array(page);
        }
        let end = offset + page.len();
        json!({"commands": page, "total": total, "offset": offset, "next_offset": (end < total).then_some(end)})
    }
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
    /// A `list_commands` step's catalog, narrowed as the tool narrows it.
    Commands(CommandQuery),
    /// A `measure {summary: true}` step's reply, as `measure_summary` shapes it.
    MeasureSummary,
}

impl Narrow {
    fn apply(&self, v: Value) -> Value {
        match self {
            Narrow::Commands(q) => q.apply(v),
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
    // Any other key would be dropped and the command run without its parameters, failing (if at
    // all) with an error about a parameter that seems to be there: `args` for `params`.
    if let Some(k) = step.as_object().into_iter().flat_map(Map::keys).find(|k| !matches!(k.as_str(), "command" | "id" | "params")) {
        if k == "expect" {
            return Err("unknown key `expect` in a step: `expect` is the batch's argument, checked after the last step".into());
        }
        let k: String = k.chars().take(64).collect();
        return Err(format!("unknown key `{k}` in a step: a step is {{command, params}}, its parameters go in `params`"));
    }
    let empty = Map::new();
    let params = step.get("params").cloned().filter(|p| !p.is_null()).unwrap_or(json!({}));
    let a = params.as_object().unwrap_or(&empty);
    let mapped = |c: &str, p: Value| Ok((c.to_string(), p, None));
    match command {
        "list_commands" => {
            let query = CommandQuery::parse(a)?;
            Ok(("engine.commands".into(), json!({}), (query != CommandQuery::default()).then_some(Narrow::Commands(query))))
        }
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
            None => Ok(("inspect.measure".into(), measure_params(a), measure_summary_wanted(a).then_some(Narrow::MeasureSummary))),
        },
        "compare_picture" => {
            // A batch step gets the numbers; the overlay PNG would be a base64 blob in text.
            let mut p = pick(a, &["reference", "view", "camera", "bodies", "crop_px", "crop_fraction", "fit", "rows"]);
            if let Some(obj) = p.as_object_mut() {
                obj.entry("overlay".to_string()).or_insert(json!(false));
            }
            mapped("inspect.compare_picture", p)
        }
        "inspect_design" => {
            let keys: &[&str] = if a.get("bodies").is_some_and(Value::is_array) { &["timeline"] } else { &["timeline", "bodies"] };
            mapped("document.inspect", inspect_params(a, a.get("measure").and_then(Value::as_bool).unwrap_or(true), keys))
        }
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
        "cancel" => Err("`cancel` is a tool, not a command: call it on its own".into()),
        // An engine command: the step goes as it is.
        _ => Ok((String::new(), Value::Null, None)),
    }
}

/// The tool names a `batch` step may not be (or is mapped from): `execute` must not wrap them.
const TOOL_STEPS: &[&str] = &[
    "batch",
    "body_topology",
    "cancel",
    "check_parameters",
    "compare_picture",
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

/// Names for an error message: at most 40 of them.
fn name_list(names: &[String]) -> String {
    let mut out = names.iter().take(40).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 40 {
        out.push_str(&format!(", … ({} in all)", names.len()));
    }
    if out.is_empty() { "none".into() } else { out }
}

/// `inspect_design` with compact output: `document.inspect`, with sketch summaries unless
/// sketches are asked for in full (by name or id, or all), and only the bodies asked for.
fn inspect_design_compact(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let measure = a.get("measure").and_then(Value::as_bool).unwrap_or(true);
    // `bodies` as a list keeps only those bodies; true, false or "names" go to document.inspect.
    if let Some(Value::String(form)) = a.get("bodies")
        && form != "names"
    {
        return Err("inspect_design: `bodies` must be a list of body names, true, false or \"names\"".into());
    }
    inspect_words(a, false)?;
    let bodies: Option<Vec<String>> = match a.get("bodies").filter(|v| v.is_array()) {
        None => None,
        Some(v) => Some(
            v.as_array()
                .and_then(|l| l.iter().map(|n| n.as_str().map(str::to_string)).collect::<Option<Vec<_>>>())
                .ok_or("inspect_design: `bodies` must be a list of body names")?,
        ),
    };
    // With `bodies`, only those are measured.
    let keys: &[&str] = if bodies.is_some() { &["timeline"] } else { &["timeline", "bodies"] };
    let mut p = inspect_params(a, measure && bodies.is_none(), keys);
    p["sketch_counts"] = json!(true);
    let mut d = b.call("document.inspect", p)?;
    if let Some(want) = bodies {
        let all: Vec<Value> = d["bodies"].as_array().cloned().unwrap_or_default();
        let names: Vec<String> = all.iter().filter_map(|x| x["name"].as_str().map(str::to_string)).collect();
        if let Some(missing) = want.iter().find(|n| !names.contains(n)) {
            return Err(format!("inspect_design: no body `{missing}` (bodies: {})", name_list(&names)));
        }
        let mut kept: Vec<Value> = all.into_iter().filter(|x| x["name"].as_str().is_some_and(|n| want.iter().any(|w| w == n))).collect();
        if measure && !want.is_empty() {
            let m = exec(b, "inspect.measure", json!({"bodies": want}))?;
            for body in &mut kept {
                let measured = m["bodies"].as_array().and_then(|l| l.iter().find(|x| x["name"] == body["name"])).cloned();
                if let (Some(Value::Object(fields)), Some(o)) = (measured, body.as_object_mut()) {
                    for (k, v) in fields {
                        o.entry(k).or_insert(v);
                    }
                }
            }
        }
        d["bodies"] = Value::Array(kept);
    }
    let mut sketches: Vec<Value> = d["sketches"].as_array().cloned().unwrap_or_default();
    // An app from before `sketch_counts` leaves out the counts: take them from each sketch.
    for x in sketches.iter_mut().filter(|x| x.get("points").is_none()) {
        if let Ok(full) = exec(b, "sketch.inspect", json!({"sketch": x["id"].clone()}))
            && let Some(o) = x.as_object_mut()
        {
            for key in ["points", "constraints"] {
                if let Some(n) = full[key].as_array().map(Vec::len) {
                    o.insert(key.into(), json!(n));
                }
            }
            if let Some(plane) = full.get("plane").or_else(|| full["sketch"].get("plane")) {
                o.insert("plane".into(), plane.clone());
            }
        }
    }
    d["sketches"] = Value::Array(sketches.clone());
    let full: Vec<Value> = match a.get("sketches") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Vec::new(),
        Some(Value::String(t)) if t == "summary" => Vec::new(),
        Some(Value::Bool(true)) => sketches.iter().map(|x| x["id"].clone()).collect(),
        Some(Value::String(t)) if t == "full" || t == "all" => sketches.iter().map(|x| x["id"].clone()).collect(),
        Some(Value::Array(list)) => {
            let mut ids = Vec::new();
            for want in list {
                let found = sketches.iter().find(|x| match want {
                    Value::String(n) => x["name"].as_str() == Some(n.as_str()) || x["id"].as_u64().is_some_and(|id| n.parse::<u64>() == Ok(id)),
                    Value::Number(_) => x["id"] == *want,
                    _ => false,
                });
                match found {
                    Some(x) => ids.push(x["id"].clone()),
                    None => {
                        let names: Vec<String> = sketches.iter().filter_map(|x| x["name"].as_str().map(str::to_string)).collect();
                        return Err(format!("inspect_design: no sketch {want} (sketches: {})", name_list(&names)));
                    }
                }
            }
            ids
        }
        Some(other) => {
            return Err(format!("inspect_design: `sketches` must be true, false, \"summary\" or a list of sketch names (got {other})"));
        }
    };
    if !full.is_empty() {
        let out: Vec<Value> = sketches
            .into_iter()
            .map(|x| {
                if full.contains(&x["id"]) {
                    exec(b, "sketch.inspect", json!({"sketch": x["id"].clone()})).unwrap_or_else(|e| json!({"id": x["id"], "error": e}))
                } else {
                    x
                }
            })
            .collect();
        d["sketches"] = Value::Array(out);
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

/// The steps of the script file `batch` was given as `path` (`None`: it was given `commands`).
/// A file it can't read, or whose steps aren't a list, fails the batch before anything is sent.
fn script_file(a: &Map<String, Value>) -> Result<Option<Vec<Value>>, String> {
    let Some(path) = a.get("path") else { return Ok(None) };
    let Some(path) = path.as_str().map(str::trim).filter(|p| !p.is_empty()) else {
        return Err("`path` must name a script file".into());
    };
    if a.contains_key("commands") {
        return Err("give `commands` or `path`, not both".into());
    }
    let text = vfs::read_to_string(path, MAX_SCRIPT_BYTES).map_err(|e| format!("script file `{path}`: {e}"))?;
    let script: Value = serde_json::from_str(&text).map_err(|e| format!("script file `{path}` is not JSON: {e}"))?;
    let steps = match script {
        Value::Array(steps) => steps,
        Value::Object(mut o) => match o.remove("commands") {
            Some(Value::Array(steps)) => steps,
            _ => return Err(format!("script file `{path}`: no `commands` array")),
        },
        _ => return Err(format!("script file `{path}`: steps must be an array or {{\"commands\": [...]}}")),
    };
    if steps.is_empty() {
        return Err(format!("script file `{path}` has no steps"));
    }
    Ok(Some(steps))
}

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
fn batch(b: &mut dyn Backend, a: &Map<String, Value>, opts: Options) -> Value {
    let expect = match Expect::parse(a) {
        Ok(e) => e,
        Err(e) => return text_result(&Value::String(e), true, opts),
    };
    match batch_run(b, a) {
        (reply, true) => text_result(&reply, true, opts),
        (reply, false) => with_expectation(b, reply, expect, opts),
    }
}

fn layout_step(step: &Value) -> bool {
    let command = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str).unwrap_or_default();
    let command = if command == "execute" {
        step.get("params").and_then(|params| params.get("command")).and_then(Value::as_str).unwrap_or_default()
    } else {
        command
    };
    command.split('.').next().is_some_and(|area| area.eq_ignore_ascii_case("panel") || area.eq_ignore_ascii_case("layout"))
}

fn atomic_layout_refusal(count: usize, rule: solidcraft_engine::panels::Rule, detail: &str) -> (Value, bool) {
    use solidcraft_engine::panels::{commands, error};
    let mut reply = commands::sequential_failure(0, None, commands::error_text(error(rule, detail)), Vec::new());
    reply["ok"] = json!(false);
    reply["applied"] = json!([]);
    // Only an atomic batch (a rollback asked for) is refused here, and nothing ran: the design is
    // where the batch started, as a rollback leaves it (#10749).
    reply["rolled_back"] = json!(true);
    reply["not_run"] = if count <= MAX_BATCH { json!((0..count).collect::<Vec<_>>()) } else { json!([]) };
    reply["not_run_count"] = json!(count);
    reply["note"] = json!("nothing was run: atomic application layout requires the authoritative backend transaction");
    (reply, true)
}

/// The batch itself: its reply and whether it failed.
fn batch_run(b: &mut dyn Backend, a: &Map<String, Value>) -> (Value, bool) {
    let from_file = match script_file(a) {
        Ok(f) => f,
        Err(e) => return (json!(e), true),
    };
    let Some(steps) = from_file.as_ref().map_or_else(|| a.get("commands").and_then(Value::as_array), Some) else {
        return (json!("`commands` must be an array of {command, params}"), true);
    };
    if steps.len() > MAX_BATCH {
        if a.get("rollback").and_then(Value::as_bool).unwrap_or(true) && steps.iter().any(layout_step) {
            return atomic_layout_refusal(steps.len(), solidcraft_engine::panels::Rule::CollectionLimit, "atomic layout batch step count cap");
        }
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
                // Nothing ran: the design is where the batch started, as a rollback leaves it (#10749).
                let reply = json!({
                    "ok": false,
                    "completed": 0,
                    "failed": {"index": i, "command": command, "error": e},
                    "rolled_back": rollback,
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
    let atomic_layout = rollback && steps.iter().any(layout_step);
    // A step the batch would refuse at its turn refuses it now, before anything runs (#5853).
    // Only where the commands run in this process: a running app may be another version, with
    // commands this one lacks, and checks the batch against its own registry when it gets it.
    if b.in_process()
        && let Some(refusal) = solidcraft_engine::batch_preflight::check(steps, b.param_check(), &[])
    {
        let mut reply = refusal.reply(steps.len(), rollback);
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
    let fits_transport = params.to_string().len().saturating_add(64 * 1024) <= solidcraft_engine::MAX_LINE;
    if fits_transport {
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
            Err(e) if solidcraft_engine::panels::commands::structured_error(&e).is_some() => return (Value::String(e), true),
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
    // Application layout is outside document checkpoints. Neither an unavailable
    // batch endpoint nor an oversized transport request authorizes a sequential
    // emulation of an atomic layout/mixed transaction.
    if atomic_layout {
        return atomic_layout_refusal(
            steps.len(),
            if fits_transport { solidcraft_engine::panels::Rule::Capability } else { solidcraft_engine::panels::Rule::CollectionLimit },
            if fits_transport {
                "authoritative atomic layout batch capability is unavailable"
            } else {
                "atomic layout batch exceeds transport size cap"
            },
        );
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

/// A `created` list longer than this (a drawing of hundreds of curves) is left out of a compact
/// reply and counted instead (`curve_count` and the like, as `point_count`): the ids are no
/// sequence an agent could derive, each kind of curve counts on its own. `sketch.inspect` has them.
const COMPACT_LIST: usize = 20;

/// A command's result without what the design already had (`execute` or `batch` with `verbose: false`): a
/// feature's full body list (its created, changed and removed bodies stay), and the sketch ids
/// a sketch command repeats outside `created`. Long `created` lists are counted instead of listed.
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
    // More new points than the new curves have ends (an offset's spline poles, #9180): agents
    // refer to those curves by id, so the points only fill the context. Keep how many there were.
    if let Some(created) = o.get_mut("created").and_then(Value::as_object_mut) {
        let count = |k: &str| created.get(k).and_then(Value::as_array).map_or(0, Vec::len);
        let (points, curves) = (count("points"), count("curves"));
        if curves > 0 && points > 2 * curves {
            created.remove("points");
            created.insert("point_count".into(), json!(points));
        }
    }
    if let Some(created) = o.get_mut("created").and_then(Value::as_object_mut) {
        for (k, count) in
            [("points", "point_count"), ("curves", "curve_count"), ("constraints", "constraint_count"), ("dimensions", "dimension_count")]
        {
            let Some(n) = created.get(k).and_then(Value::as_array).map(Vec::len).filter(|n| *n > COMPACT_LIST) else { continue };
            created.remove(k);
            created.insert(count.into(), json!(n));
        }
    }
    compact_moved_points(o);
    v
}

/// `sketch.move_points` answers one `{point, to, at, reached}` row per move (#15864). A compact
/// reply counts the moves, says whether all of them reached their targets, and lists only the
/// moves that did not, under `not_reached`. Its empty `created` is left out too. A `moved` that
/// is not such a list (a count, a flag or names, as other commands answer) stays as it is.
fn compact_moved_points(o: &mut Map<String, Value>) {
    let Some(rows) = o.get("moved").and_then(Value::as_array) else { return };
    if rows.is_empty() || !rows.iter().all(|r| r.get("reached").is_some_and(Value::is_boolean) && r.get("to").is_some()) {
        return;
    }
    let count = rows.len();
    let missed: Vec<Value> = rows.iter().filter(|r| r.get("reached") != Some(&Value::Bool(true))).cloned().collect();
    o.remove("moved");
    o.insert("moved_count".into(), json!(count));
    o.insert("reached".into(), json!(missed.is_empty()));
    if !missed.is_empty() {
        let missed: Vec<Value> = missed
            .into_iter()
            .map(|mut r| {
                if let Some(m) = r.as_object_mut() {
                    m.remove("reached");
                }
                r
            })
            .collect();
        o.insert("not_reached".into(), Value::Array(missed));
    }
    let empty = |c: &Value| c.as_object().is_some_and(|m| m.values().all(|x| x.as_array().is_some_and(Vec::is_empty)));
    if o.get("created").is_some_and(empty) {
        o.remove("created");
    }
}

/// `check_parameters`: try parameter values and put the design back (#3380). The checkpoint is
/// taken before anything changes; whatever happens after it, the design is restored, and a
/// restore that fails is an error saying the values are still in. Every agent session that
/// changed a parameter to see its effect spent three to five calls on this loop, and one saved
/// the design with the test values still in it.
fn check_parameters(b: &mut dyn Backend, a: &Map<String, Value>, opts: Options) -> Value {
    let (changes, reports, narrow) = match check_parameters_args(a) {
        Ok(x) => x,
        Err(e) => return text_result(&Value::String(e), true, opts),
    };
    // A name the design has no parameter of is refused before the checkpoint, so nothing
    // changes: `parameters.change` would make one.
    if let Err(e) = known_parameters(b, &changes) {
        return text_result(&Value::String(e), true, opts);
    }
    let Some(id) = exec(b, "edit.checkpoint", json!({})).ok().and_then(|v| v["checkpoint"].as_u64()) else {
        let e = "check_parameters: this app has no edit.checkpoint to put the design back with; use set_parameter and measure, and set the values back yourself";
        return text_result(&json!(e), true, opts);
    };
    let outcome = try_parameters(b, &changes, &reports, &narrow);
    let restore = exec(b, "edit.restore_checkpoint", json!({"checkpoint": id}));
    match (outcome, restore) {
        (Ok(mut reply), Ok(_)) => {
            reply["restored"] = json!(true);
            text_result(&reply, false, opts)
        }
        (Err(e), Ok(_)) => text_result(&json!({"error": e, "restored": true, "note": "the design is back as it was"}), true, opts),
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
            text_result(&reply, true, opts)
        }
    }
}

/// How small `check_parameters`'s reports are: only these bodies (`None`: all), and `measure`
/// summarised as the measure tool's `summary: true`.
struct ReportScope {
    bodies: Option<Vec<String>>,
    summary: bool,
}

/// `check_parameters`'s arguments, checked before anything is sent: the values to try (a
/// number or an expression each, as `set_parameter` takes its `value`), what to report and how
/// small.
fn check_parameters_args(a: &Map<String, Value>) -> Result<(Map<String, Value>, Vec<&'static str>, ReportScope), String> {
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
    let bodies = match a.get("bodies").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            let names = v
                .as_array()
                .and_then(|l| l.iter().map(|n| n.as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string)).collect::<Option<Vec<_>>>())
                .filter(|l| !l.is_empty())
                .ok_or("check_parameters: `bodies` must be a list of body names")?;
            Some(names)
        }
    };
    let summary = match a.get("summary").filter(|v| !v.is_null()) {
        None => false,
        Some(v) => v.as_bool().ok_or("check_parameters: `summary` must be true or false")?,
    };
    Ok((changes, reports, ReportScope { bodies, summary }))
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
fn try_parameters(b: &mut dyn Backend, changes: &Map<String, Value>, reports: &[&str], scope: &ReportScope) -> Result<Value, String> {
    let (values, recomputed) = apply_parameters(b, changes)?;
    let mut reply = json!({"set": changes, "values": values, "recomputed": recomputed});
    // The bodies and each feature's state: one cheap read serves `errors` and `cylinders`.
    let design = if reports.iter().any(|r| *r != "measure") || scope.bodies.is_some() {
        Some(b.call("document.inspect", json!({"measure": false}))?)
    } else {
        None
    };
    // A body named in `bodies` that the design doesn't have (now) is an error that names them.
    if let (Some(want), Some(d)) = (&scope.bodies, &design) {
        let names: Vec<String> = d["bodies"].as_array().into_iter().flatten().filter_map(|body| body["name"].as_str()).map(str::to_string).collect();
        if let Some(missing) = want.iter().find(|n| !names.contains(n)) {
            return Err(format!("check_parameters: no body `{missing}` (bodies: {})", name_list(&names)));
        }
    }
    for r in reports {
        match *r {
            "measure" => {
                let p = match &scope.bodies {
                    Some(names) => json!({"bodies": names}),
                    None => json!({}),
                };
                let m = exec(b, "inspect.measure", p)?;
                reply["measure"] = if scope.summary { measure_summary(m) } else { m };
            }
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
                    let mut bodies: Vec<String> =
                        d["bodies"].as_array().into_iter().flatten().filter_map(|body| body["name"].as_str()).map(str::to_string).collect();
                    if let Some(want) = &scope.bodies {
                        bodies.retain(|n| want.contains(n));
                    }
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
        Ok((png, info)) => solidcraft_mcp_server::image_result(&png, "image/png", info.to_string()),
        Err(e) => text_result(&Value::String(e), true, Options::default()),
    }
}

/// `compare_picture`: run the engine comparison and hand the overlay PNG back as an MCP
/// image block, the numbers as text — an agent sees the picture, not a base64 blob.
fn compare_picture(b: &mut dyn Backend, a: &Map<String, Value>, opts: Options) -> Value {
    let mut params = pick(a, &["reference", "view", "camera", "bodies", "crop_px", "crop_fraction", "fit", "rows"]);
    params["overlay"] = json!(true);
    match exec(b, "inspect.compare_picture", params) {
        Ok(v) => {
            let overlay = v.get("overlay_png").and_then(Value::as_str).unwrap_or_default().to_string();
            let mut info = v;
            if let Some(obj) = info.as_object_mut() {
                obj.remove("overlay_png");
            }
            let mut content = vec![json!({"type": "text", "text": info.to_string()})];
            if !overlay.is_empty() {
                content.insert(0, json!({"type": "image", "data": overlay, "mimeType": "image/png"}));
            }
            json!({"content": content})
        }
        Err(e) => text_result(&Value::String(e), true, opts),
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


/// The largest coordinate or length (mm) the inspection and query tools take.
const MAX_MM: f64 = 1e6;

/// A finite `[x, y, z]` argument within ±[`MAX_MM`].
fn vec3_arg(a: &Map<String, Value>, k: &str) -> Result<Option<[f64; 3]>, String> {
    let Some(v) = a.get(k) else { return Ok(None) };
    let xs: Vec<f64> = v.as_array().map(|l| l.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    match xs.as_slice() {
        [x, y, z] if xs.iter().all(|c| c.is_finite() && c.abs() <= MAX_MM) => Ok(Some([*x, *y, *z])),
        _ => Err(format!("`{k}` must be [x, y, z] (finite numbers)")),
    }
}

/// A finite number argument in `lo..=hi`.
fn num_arg(a: &Map<String, Value>, k: &str, lo: f64, hi: f64) -> Result<Option<f64>, String> {
    match a.get(k) {
        None => Ok(None),
        Some(v) => {
            v.as_f64().filter(|x| x.is_finite() && (lo..=hi).contains(x)).map(Some).ok_or_else(|| format!("`{k}` must be a number from {lo} to {hi}"))
        }
    }
}

/// A whole-number argument in `lo..=hi`.
fn count_arg(a: &Map<String, Value>, k: &str, lo: u64, hi: u64) -> Result<Option<u64>, String> {
    match a.get(k) {
        None => Ok(None),
        Some(v) => v.as_u64().filter(|n| (lo..=hi).contains(n)).map(Some).ok_or_else(|| format!("`{k}` must be a whole number from {lo} to {hi}")),
    }
}

fn unit(v: [f64; 3], k: &str) -> Result<[f64; 3], String> {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n.is_nan() || n <= 1e-12 {
        return Err(format!("`{k}` must not be the zero vector"));
    }
    Ok([v[0] / n, v[1] / n, v[2] / n])
}



fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot(d, d).sqrt()
}

/// A finite `[x, y, z]` value.
fn vec3_value(v: &Value) -> Option<[f64; 3]> {
    let xs: Vec<f64> = v.as_array()?.iter().filter_map(Value::as_f64).collect();
    match xs.as_slice() {
        [x, y, z] if xs.iter().all(|c| c.is_finite()) => Some([*x, *y, *z]),
        _ => None,
    }
}

fn query_schema() -> Value {
    let v3 = |d: &str| json!({"type": "array", "items": {"type": "number"}, "description": d});
    let n = |d: &str| json!({"type": "number", "description": d});
    json!({
        "body": string("Body name"),
        "kind": {"type": "string", "enum": ["face", "edge"], "description": "What to find (default face)"},
        "surface": {"type": "string", "enum": ["plane", "cylinder", "other"], "description": "Faces: surface type"},
        "normal": v3("Faces: planar faces whose outward normal is within `angle_tol_deg` of this direction"),
        "axis": v3("Faces: cylinders whose axis is parallel to this direction (either way)"),
        "radius": n("Faces: cylinders of this radius (± `tol`)"),
        "radius_min": n("Faces: cylinders of at least this radius"),
        "radius_max": n("Faces: cylinders of at most this radius"),
        "area_min": n("Faces: area at least (mm²; areas are model.faces', a few percent low on faces bounded by circles)"),
        "area_max": n("Faces: area at most (mm²)"),
        "curve": {"type": "string", "enum": ["line", "arc", "circle"], "description": "Edges: curve type (circle: closed)"},
        "direction": v3("Edges: straight edges parallel to this direction (either way)"),
        "length": n("Edges: this length (± `tol`)"),
        "length_min": n("Edges: length at least"),
        "length_max": n("Edges: length at most"),
        "of_face": string("Edges: only the edges bounding this face (a face name from find_faces or body_topology)"),
        "extreme": {"type": "object", "description": "Keep what lies furthest along a direction: {\"direction\": [x,y,z], \"side\"?: \"max\" (default) | \"min\"}, within `tol`"},
        "near": v3("Sort by distance from this point, nearest first"),
        "tol": n("Length tolerance in mm (default 0.001)"),
        "angle_tol_deg": n("Direction tolerance in degrees (default 1)"),
        "limit": {"type": "integer", "description": "Most matches returned (default 50, at most 1000)"},
    })
}

/// One face or edge with what the filters look at.
struct Item {
    raw: Value,
    /// Where it is for `extreme` and `near` (a face's centroid, an edge's midpoint).
    at: [f64; 3],
    /// What a command takes to pick it.
    pick: [f64; 3],
    kind: &'static str,
    /// Plane normal, cylinder axis or line direction.
    dir: Option<[f64; 3]>,
    radius: Option<f64>,
    /// Area (faces) or length (edges).
    size: f64,
}

/// The matching faces or edges, after `limit`, the number before it and the distinct cylinders.
fn query_items(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<(Vec<Item>, usize, usize), String> {
    let allowed = query_schema();
    if let Some(k) = a.keys().find(|k| allowed.get(k.as_str()).is_none()) {
        return Err(format!("unknown query argument `{k}`"));
    }
    let body = str_arg(a, "body").ok_or("`body` is required")?;
    let edges = match str_arg(a, "kind").unwrap_or("face") {
        "face" => false,
        "edge" => true,
        other => return Err(format!("unknown kind `{other}` (face or edge)")),
    };
    let tol = num_arg(a, "tol", 0.0, MAX_MM)?.unwrap_or(1e-3);
    let cos_tol = num_arg(a, "angle_tol_deg", 0.0, 90.0)?.unwrap_or(1.0).to_radians().cos();
    let limit = count_arg(a, "limit", 1, 1000)?.unwrap_or(50) as usize;
    let unit_arg = |k: &str| vec3_arg(a, k)?.map(|v| unit(v, k)).transpose();
    let (normal, axis, direction, near) = (unit_arg("normal")?, unit_arg("axis")?, unit_arg("direction")?, vec3_arg(a, "near")?);
    let range = |k: &str| num_arg(a, k, 0.0, MAX_MM * MAX_MM);
    let (r, r_min, r_max) = (range("radius")?, range("radius_min")?, range("radius_max")?);
    let (len, len_min, len_max) = (range("length")?, range("length_min")?, range("length_max")?);
    let (area_min, area_max) = (range("area_min")?, range("area_max")?);
    let extreme = match a.get("extreme") {
        None => None,
        Some(e) => {
            let d = e.get("direction").and_then(vec3_value).ok_or("`extreme.direction` must be [x, y, z]")?;
            let max = match e.get("side").and_then(Value::as_str).unwrap_or("max") {
                "max" => true,
                "min" => false,
                other => return Err(format!("unknown `extreme.side` `{other}` (max or min)")),
            };
            Some((unit(d, "extreme.direction")?, max))
        }
    };

    let (cmd, key) = if edges { ("model.edges", "edges") } else { ("model.faces", "faces") };
    let list = exec(b, cmd, json!({"body": body}))?.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut items: Vec<Item> = list
        .into_iter()
        .filter_map(|raw| {
            if edges {
                let (s, e, m) = (vec3_value(raw.get("start")?)?, vec3_value(raw.get("end")?)?, vec3_value(raw.get("mid")?)?);
                let length = raw.get("length").and_then(Value::as_f64)?;
                let chord = dist(s, e);
                let kind = if (length - chord).abs() <= 1e-6 * length.max(1.0) {
                    "line"
                } else if chord <= 1e-9 * length.max(1.0) {
                    "circle"
                } else {
                    "arc"
                };
                let dir = (kind == "line").then(|| unit([e[0] - s[0], e[1] - s[1], e[2] - s[2]], "edge").ok()).flatten();
                Some(Item { raw, at: m, pick: m, kind, dir, radius: None, size: length })
            } else {
                let at = vec3_value(raw.get("centroid")?)?;
                let pick = raw.get("point").and_then(vec3_value).unwrap_or(at);
                let area = raw.get("area").and_then(Value::as_f64)?;
                let normal = raw.get("plane_normal").and_then(vec3_value);
                let cyl = raw.get("cylinder");
                let (kind, dir, radius) = match (normal, cyl) {
                    (Some(n), _) => ("plane", Some(n), None),
                    (None, Some(c)) => ("cylinder", c.get("axis").and_then(vec3_value), c.get("radius").and_then(Value::as_f64)),
                    _ => ("other", None, None),
                };
                Some(Item { raw, at, pick, kind, dir, radius, size: area })
            }
        })
        .collect();

    let parallel = |d: Option<[f64; 3]>, want: [f64; 3], signed: bool| {
        d.and_then(|d| unit(d, "").ok()).is_some_and(|d| if signed { dot(d, want) >= cos_tol } else { dot(d, want).abs() >= cos_tol })
    };
    let in_range = |v: Option<f64>, eq: Option<f64>, lo: Option<f64>, hi: Option<f64>| {
        eq.is_none_or(|x| v.is_some_and(|v| (v - x).abs() <= tol))
            && lo.is_none_or(|x| v.is_some_and(|v| v >= x - tol))
            && hi.is_none_or(|x| v.is_some_and(|v| v <= x + tol))
    };
    let surface = str_arg(a, "surface");
    let curve = str_arg(a, "curve");
    let of_face = str_arg(a, "of_face");
    items.retain(|it| {
        let name = it.raw.get("name").and_then(Value::as_str).unwrap_or("");
        if edges {
            curve.is_none_or(|c| c == it.kind)
                && direction.is_none_or(|d| parallel(it.dir, d, false))
                && in_range(Some(it.size), len, len_min, len_max)
                && of_face.is_none_or(|f| name.starts_with(&format!("{f}|")) || name.ends_with(&format!("|{f}")))
        } else {
            surface.is_none_or(|s| s == it.kind)
                && normal.is_none_or(|n| it.kind == "plane" && parallel(it.dir, n, true))
                && axis.is_none_or(|x| it.kind == "cylinder" && parallel(it.dir, x, false))
                && in_range(it.radius, r, r_min, r_max)
                && in_range(Some(it.size), None, area_min, area_max)
        }
    });
    if let Some((d, max)) = extreme {
        let key = |it: &Item| if max { dot(it.at, d) } else { -dot(it.at, d) };
        if let Some(best) = items.iter().map(key).reduce(f64::max) {
            items.retain(|it| best - key(it) <= tol);
        }
    }
    if let Some(p) = near {
        items.sort_by(|x, y| dist(x.pick, p).total_cmp(&dist(y.pick, p)));
    }
    // Cylinders by axis line and radius: one hole is one, whatever faces its wall is split into.
    let mut cylinders: Vec<([f64; 3], [f64; 3], f64)> = Vec::new();
    for it in items.iter().filter(|it| it.kind == "cylinder") {
        let c = it.raw.get("cylinder");
        let (Some(o), Some(d), Some(r)) = (c.and_then(|c| c.get("origin")).and_then(vec3_value), it.dir.and_then(|d| unit(d, "").ok()), it.radius)
        else {
            continue;
        };
        let same = |(o2, d2, r2): &([f64; 3], [f64; 3], f64)| {
            let off = [o[0] - o2[0], o[1] - o2[1], o[2] - o2[2]];
            let along = dot(off, *d2);
            let across = (dot(off, off) - along * along).max(0.0).sqrt();
            (r - r2).abs() <= tol && dot(d, *d2).abs() >= cos_tol && across <= tol.max(1e-6)
        };
        if !cylinders.iter().any(same) {
            cylinders.push((o, d, r));
        }
    }
    let total = items.len();
    items.truncate(limit);
    Ok((items, total, cylinders.len()))
}

fn find_faces(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let (items, total, cylinders) = query_items(b, a)?;
    let matches: Vec<Value> = items
        .iter()
        .map(|it| {
            let mut m = json!({"index": it.raw.get("index"), "name": it.raw.get("name"), "type": it.kind, "pick": it.pick});
            if it.kind == "line" || it.kind == "arc" || it.kind == "circle" {
                m["length"] = json!(it.size);
                m["start"] = it.raw.get("start").cloned().unwrap_or(Value::Null);
                m["end"] = it.raw.get("end").cloned().unwrap_or(Value::Null);
            } else {
                m["area"] = json!(it.size);
                m["centroid"] = json!(it.at);
            }
            if let Some(d) = it.dir {
                m[if it.kind == "plane" {
                    "normal"
                } else if it.kind == "cylinder" {
                    "axis"
                } else {
                    "direction"
                }] = json!(d);
            }
            if let Some(r) = it.radius {
                m["radius"] = json!(r);
            }
            m
        })
        .collect();
    Ok(json!({
        "body": str_arg(a, "body"),
        "kind": str_arg(a, "kind").unwrap_or("face"),
        "count": total,
        "returned": matches.len(),
        "distinct_cylinders": cylinders,
        "matches": matches,
    }))
}

const MAX_CHECKS: usize = 100;

fn verify_design(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let checks = a.get("checks").and_then(Value::as_array).ok_or("`checks` must be an array")?;
    if checks.is_empty() || checks.len() > MAX_CHECKS {
        return Err(format!("`checks` must hold 1 to {MAX_CHECKS} checks"));
    }
    let results: Vec<Value> = checks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let kind = c.get("check").and_then(Value::as_str).unwrap_or("").to_owned();
            let mut row = match c.as_object() {
                Some(c) => verify_one(b, &kind, c).unwrap_or_else(|e| json!({"passed": false, "error": e})),
                None => json!({"passed": false, "error": "a check must be an object"}),
            };
            row["index"] = json!(i);
            row["check"] = json!(kind);
            row
        })
        .collect();
    let failed = results.iter().filter(|r| r["passed"] != json!(true)).count();
    Ok(json!({"passed": failed == 0, "failed": failed, "total": results.len(), "checks": results}))
}

/// `equals` (with relative `tol_rel` or absolute `tol`), `min` and `max` against `v`.
fn compare(c: &Map<String, Value>, v: f64, default_rel: f64) -> Result<Value, String> {
    let num = |k: &str| match c.get(k) {
        None => Ok(None),
        Some(x) => x.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| format!("`{k}` must be a finite number")),
    };
    let (eq, lo, hi) = (num("equals")?, num("min")?, num("max")?);
    if eq.is_none() && lo.is_none() && hi.is_none() {
        return Err("give `equals`, `min` or `max`".into());
    }
    let rel = num("tol_rel")?.unwrap_or(default_rel).abs();
    let abs = num("tol")?.unwrap_or(0.0).abs();
    let passed =
        eq.is_none_or(|x| (v - x).abs() <= abs.max(rel * x.abs().max(1e-12))) && lo.is_none_or(|x| v >= x - abs) && hi.is_none_or(|x| v <= x + abs);
    Ok(json!({"passed": passed, "measured": v, "expected": {"equals": eq, "min": lo, "max": hi}}))
}

fn measured_body(b: &mut dyn Backend, body: Option<&str>) -> Result<Value, String> {
    let m = exec(b, "inspect.measure", body.map_or(json!({}), |n| json!({"bodies": [n]})))?;
    match body {
        Some(_) => m.get("bodies").and_then(Value::as_array).and_then(|l| l.first()).cloned().ok_or_else(|| "no such body".to_string()),
        None => Ok(m),
    }
}

fn verify_one(b: &mut dyn Backend, kind: &str, c: &Map<String, Value>) -> Result<Value, String> {
    let body = str_arg(c, "body");
    match kind {
        "volume" | "mass" => {
            let m = measured_body(b, body)?;
            let src = if body.is_some() { &m } else { m.get("total").unwrap_or(&Value::Null) };
            let key = if kind == "volume" { "volume_mm3" } else { "mass_g" };
            let v = src.get(key).and_then(Value::as_f64).ok_or_else(|| format!("no {key} measured"))?;
            compare(c, v, 1e-3)
        }
        "body_count" => {
            let n = exec(b, "inspect.measure", json!({}))?.get("body_count").and_then(Value::as_f64).ok_or("no body count")?;
            compare(c, n, 0.0)
        }
        "bbox" => {
            let m = measured_body(b, body)?;
            // One body's box, or the box around every body.
            let boxes: Vec<Value> = if body.is_some() { vec![m] } else { m.get("bodies").and_then(Value::as_array).cloned().unwrap_or_default() };
            let corners = |k: &str| boxes.iter().filter_map(|x| x.get("bbox").and_then(|bb| bb.get(k)).and_then(vec3_value)).collect::<Vec<_>>();
            let fold = |l: Vec<[f64; 3]>, pick: fn(f64, f64) -> f64| l.into_iter().reduce(|p, q| [0, 1, 2].map(|i| pick(p[i], q[i])));
            let (Some(lo), Some(hi)) = (fold(corners("min"), f64::min), fold(corners("max"), f64::max)) else {
                return Err("no bounding box".into());
            };
            let size = [0, 1, 2].map(|i| hi[i] - lo[i]);
            let tol = num_arg(c, "tol", 0.0, MAX_MM)?.unwrap_or(0.01);
            let want = vec3_arg(c, "size")?;
            let fits = vec3_arg(c, "fits")?;
            if want.is_none() && fits.is_none() {
                return Err("give `size` or `fits`".into());
            }
            let passed =
                want.is_none_or(|w| (0..3).all(|i| (size[i] - w[i]).abs() <= tol)) && fits.is_none_or(|f| (0..3).all(|i| size[i] <= f[i] + tol));
            Ok(json!({"passed": passed, "measured": size, "expected": {"size": want, "fits": fits}}))
        }
        "timeline_ok" => {
            let d = b.call("document.inspect", json!({"measure": false}))?;
            let failing: Vec<Value> = d
                .get("timeline")
                .and_then(Value::as_array)
                .map(|t| {
                    t.iter()
                        .filter(|f| f.get("error").is_some_and(|e| !e.is_null()))
                        .map(|f| json!({"name": f.get("name"), "error": f.get("error")}))
                        .collect()
                })
                .unwrap_or_default();
            Ok(json!({"passed": failing.is_empty(), "measured": failing}))
        }
        "clearance" => {
            let mut args = Map::new();
            args.insert("body_a".into(), c.get("a").cloned().ok_or("`a` is required")?);
            args.insert("body_b".into(), c.get("b").cloned().ok_or("`b` is required")?);
            if let Some(m) = c.get("min_mm") {
                args.insert("min_clearance_mm".into(), m.clone());
            }
            let r = dfm_clearance_check(b, &args)?;
            Ok(json!({"passed": r["passed"], "measured": r["measured_gap_mm"], "expected": {"min": r["required_gap_mm"]}, "message": r["advice"]}))
        }
        "no_interference" => {
            let p = c.get("bodies").map_or(json!({}), |v| json!({"bodies": v}));
            let r = exec(b, "inspect.interference", p)?;
            let hits = r.get("interferences").cloned().unwrap_or(Value::Null);
            let unchecked = r.get("unchecked").and_then(Value::as_array).is_some_and(|u| !u.is_empty());
            let clear = hits.as_array().is_some_and(Vec::is_empty);
            Ok(json!({"passed": clear && !unchecked, "measured": hits, "unchecked": r.get("unchecked")}))
        }
        "count" => {
            let q = c.get("query").and_then(Value::as_object).ok_or("`query` must be an object of find_faces arguments")?;
            let (_, total, cylinders) = query_items(b, q)?;
            let n = match c.get("of").and_then(Value::as_str).unwrap_or("matches") {
                "matches" => total,
                "cylinders" => cylinders,
                other => return Err(format!("unknown `of` `{other}` (matches or cylinders)")),
            };
            compare(c, n as f64, 0.0)
        }
        "" => Err("`check` is required".into()),
        other => Err(format!("unknown check `{other}`")),
    }
}

/// The box around every body: (min, max).
fn design_box(b: &mut dyn Backend) -> Result<([f64; 3], [f64; 3]), String> {
    let m = exec(b, "inspect.measure", json!({}))?;
    let bodies = m.get("bodies").and_then(Value::as_array).cloned().unwrap_or_default();
    let corner = |k: &str| -> Vec<[f64; 3]> { bodies.iter().filter_map(|x| x.get("bbox").and_then(|bb| bb.get(k)).and_then(vec3_value)).collect() };
    let fold = |l: Vec<[f64; 3]>, pick: fn(f64, f64) -> f64| l.into_iter().reduce(|p, q| [0, 1, 2].map(|i| pick(p[i], q[i])));
    match (fold(corner("min"), f64::min), fold(corner("max"), f64::max)) {
        (Some(lo), Some(hi)) => Ok((lo, hi)),
        _ => Err("there are no bodies to draw".into()),
    }
}

fn export_views(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    use solidcraft_viewport::StandardView;
    let path = str_arg(a, "path").map(str::trim).filter(|p| !p.is_empty()).ok_or("`path` is required")?;
    let stem = path.strip_suffix(".dxf").or_else(|| path.strip_suffix(".DXF")).unwrap_or(path);
    let views: Vec<String> = match a.get("views") {
        None => ["front", "top", "right", "iso"].map(String::from).to_vec(),
        Some(v) => v.as_array().map(|l| l.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default(),
    };
    if views.is_empty() || views.len() > 7 {
        return Err("`views` must name 1 to 7 views".into());
    }
    let hidden = str_arg(a, "hidden").unwrap_or("dashed");
    let hidden = match hidden {
        "dashed" | "omit" => hidden,
        other => return Err(format!("unknown `hidden` `{other}` (dashed or omit)")),
    };
    let tolerance = num_arg(a, "tolerance", 0.0001, 1.0)?.unwrap_or(0.01);
    let (lo, hi) = design_box(b)?;
    let target = [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0);
    let size = [0, 1, 2].map(|i| hi[i] - lo[i]);
    // Far enough to be outside the bodies; a parallel view looks the same from any distance.
    let distance = 10.0 + 3.0 * dot(size, size).sqrt();
    let mut written = Vec::new();
    for (i, name) in views.iter().enumerate() {
        if views[..i].contains(name) {
            return Err(format!("`views` names `{name}` twice"));
        }
        let view = StandardView::parse(name).ok_or_else(|| format!("unknown view `{name}`"))?;
        let (yaw, pitch) = view.angles();
        let file = format!("{stem}_{name}.dxf");
        let camera = json!({"target": target, "yaw": yaw, "pitch": pitch, "distance": distance, "fov": 0.0});
        let r = exec(b, "file.export_view_dxf", json!({"camera": camera, "path": file, "hidden": hidden, "tolerance": tolerance}))
            .map_err(|e| if written.is_empty() { e } else { format!("{e} (already written: {})", Value::Array(written.clone())) })?;
        let seen = match name.as_str() {
            "front" | "back" => Some([size[0], size[2]]),
            "left" | "right" => Some([size[1], size[2]]),
            "top" | "bottom" => Some([size[0], size[1]]),
            _ => None,
        };
        written.push(json!({
            "view": name,
            "path": r.get("path").cloned().unwrap_or(json!(file)),
            "size_mm": seen,
            "hidden_edges": r.get("hidden"),
            "segments": r.get("stats").and_then(|s| s.get("segments")),
        }));
    }
    Ok(
        json!({"views": written, "units": "mm", "projection": "orthographic", "hidden": hidden, "tolerance": tolerance, "bbox": {"min": lo, "max": hi}}),
    )
}

fn filament_density(material: &str) -> Option<f64> {
    Some(match material {
        "PLA" => 1.24,
        "PETG" => 1.27,
        "ABS" => 1.04,
        "ASA" => 1.07,
        "TPU" => 1.21,
        "PA" => 1.14,
        _ => return None,
    })
}

fn fdm_print_estimate(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let material = str_arg(a, "material").unwrap_or("PLA");
    let density = match num_arg(a, "density_g_cm3", 0.1, 30.0)? {
        Some(d) => d,
        None => filament_density(material).ok_or_else(|| format!("unknown material `{material}` (PLA, PETG, ABS, ASA, TPU or PA)"))?,
    };
    let infill = num_arg(a, "infill_percent", 0.0, 100.0)?.unwrap_or(15.0) / 100.0;
    let wall = num_arg(a, "wall_mm", 0.0, 100.0)?.unwrap_or(1.2);
    let dia = num_arg(a, "filament_diameter_mm", 0.1, 10.0)?.unwrap_or(1.75);
    let build = vec3_arg(a, "build_volume_mm")?.unwrap_or([220.0, 220.0, 250.0]);
    if build.iter().any(|d| *d <= 0.0) {
        return Err("`build_volume_mm` must be three positive sizes".into());
    }
    let p = match a.get("bodies") {
        Some(v) => json!({"bodies": v}),
        None => json!({}),
    };
    let m = exec(b, "inspect.measure", p)?;
    let bodies = m.get("bodies").and_then(Value::as_array).cloned().unwrap_or_default();
    if bodies.is_empty() {
        return Err("no bodies to print".into());
    }
    let mut sorted_build = build;
    sorted_build.sort_by(f64::total_cmp);
    let filament_area = std::f64::consts::PI * (dia / 2.0).powi(2);
    let (mut total_mm3, mut all_fit) = (0.0, true);
    let mut rows = Vec::new();
    for body in &bodies {
        let name = body.get("name").cloned().unwrap_or(Value::Null);
        let volume = body.get("volume_mm3").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0);
        let area = body.get("area_mm2").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0);
        let corner = |k: &str| body.get("bbox").and_then(|bb| vec3_value(bb.get(k)?));
        let size = match (corner("min"), corner("max")) {
            (Some(lo), Some(hi)) => [0, 1, 2].map(|i| (hi[i] - lo[i]).max(0.0)),
            _ => return Err(format!("body {name} has no bounding box")),
        };
        let shell = (area * wall).min(volume);
        let printed = shell + (volume - shell) * infill;
        total_mm3 += printed;
        let as_placed = (0..3).all(|i| size[i] <= build[i]);
        let mut sorted = size;
        sorted.sort_by(f64::total_cmp);
        let turned = (0..3).all(|i| sorted[i] <= sorted_build[i]);
        all_fit &= turned;
        rows.push(json!({
            "body": name,
            "size_mm": size,
            "solid_volume_mm3": volume,
            "printed_volume_mm3": printed,
            "mass_g": printed / 1000.0 * density,
            "filament_m": printed / filament_area / 1000.0,
            "fits_as_placed": as_placed,
            "fits_turned": turned,
        }));
    }
    Ok(json!({
        "material": if a.contains_key("density_g_cm3") { "custom" } else { material },
        "density_g_cm3": density,
        "infill_percent": infill * 100.0,
        "wall_mm": wall,
        "build_volume_mm": build,
        "bodies": rows,
        "total": {
            "printed_volume_mm3": total_mm3,
            "mass_g": total_mm3 / 1000.0 * density,
            "filament_m": total_mm3 / filament_area / 1000.0,
            "all_fit": all_fit,
        },
        "note": "Estimate: shell + infill only; supports, brims, top/bottom layers and purge are not counted.",
    }))
}

fn dfm_clearance_check(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let body_a = str_arg(a, "body_a").ok_or("`body_a` is required")?;
    let body_b = str_arg(a, "body_b").ok_or("`body_b` is required")?;
    let min = a.get("min_clearance_mm").and_then(Value::as_f64).unwrap_or(0.25);
    if !min.is_finite() || !(0.0..=1e6).contains(&min) {
        return Err("`min_clearance_mm` must be a number from 0 to 1e6".into());
    }
    let res = exec(b, "inspect.clearance", json!({"bodies": [body_a], "against": [body_b], "clearance": min.max(10.0), "nearest": 1}))?;
    if let Some(u) = res.get("unchecked").and_then(Value::as_array).and_then(|l| l.first()) {
        return Err(format!("the clearance between `{body_a}` and `{body_b}` could not be worked out: {}", u["reason"]));
    }
    if let Some(o) = res.get("overlaps").and_then(Value::as_array).and_then(|l| l.first()) {
        let vol = o.get("volume_mm3").and_then(Value::as_f64).unwrap_or(0.0);
        return Ok(json!({
            "passed": false,
            "measured_gap_mm": 0.0,
            "required_gap_mm": min,
            "is_colliding": true,
            "advice": format!("Bodies '{body_a}' and '{body_b}' collide with overlap volume {vol:.4} mm³. Add clearance/offset of at least {min:.3} mm to separate them."),
        }));
    }
    let near = res.get("near").and_then(Value::as_array).and_then(|l| l.first()).and_then(|n| n.get("distance_mm")).and_then(Value::as_f64);
    let nearest = || {
        res.get("nearest")
            .and_then(|m| m.get(body_a))
            .and_then(Value::as_array)
            .and_then(|l| l.iter().find(|n| n.get("body").and_then(Value::as_str) == Some(body_b)))
            .and_then(|n| n.get("distance_mm"))
            .and_then(Value::as_f64)
    };
    let gap = near.or_else(nearest).ok_or_else(|| format!("no distance between `{body_a}` and `{body_b}` was measured"))?;
    let passed = gap >= min;
    let advice = if passed {
        format!("Clearance test PASSED: measured gap {gap:.3} mm meets or exceeds the required {min:.3} mm.")
    } else {
        format!(
            "Clearance test FAILED: measured gap {gap:.3} mm is below required {min:.3} mm. Widen gap by at least {:.3} mm to prevent binding when 3D printing (FDM).",
            min - gap
        )
    };
    Ok(json!({"passed": passed, "measured_gap_mm": gap, "required_gap_mm": min, "is_colliding": false, "advice": advice}))
}

fn cad_status(b: &mut dyn Backend, _a: &Map<String, Value>) -> Result<Value, String> {
    let insp = b.call("document.inspect", json!({"measure": false}))?;
    let timeline = insp.get("timeline").and_then(Value::as_array);
    let bodies = insp.get("bodies").and_then(Value::as_array);
    let sketches = insp.get("sketches").and_then(Value::as_array);
    let params = insp.get("params").and_then(Value::as_array);

    let feature_count = timeline.map(Vec::len).unwrap_or(0);
    let body_count = bodies.map(Vec::len).unwrap_or(0);
    let sketch_count = sketches.map(Vec::len).unwrap_or(0);
    let parameter_count = params.map(Vec::len).unwrap_or(0);

    let failing_features: Vec<Value> = timeline
        .map(|a| {
            a.iter()
                .filter(|f| f.get("error").is_some_and(|e| !e.is_null()))
                .map(|f| {
                    json!({
                        "id": f.get("id"),
                        "name": f.get("name"),
                        "type": f.get("type"),
                        "error": f.get("error"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let backend_mode = if b.has_ui() { "remote_gui" } else { "headless" };
    let design_path = b.design_path().unwrap_or_else(|| insp.get("name").and_then(Value::as_str).unwrap_or("Untitled").to_string());

    Ok(json!({
        "status": "ready",
        "backend": backend_mode,
        "connected_to_gui": b.has_ui(),
        "design": design_path,
        "units": insp.get("units").unwrap_or(&json!("mm")),
        "feature_count": feature_count,
        "body_count": body_count,
        "sketch_count": sketch_count,
        "parameter_count": parameter_count,
        "active_sketch": insp.get("active_sketch"),
        "undo_depth": insp.get("undo_depth").unwrap_or(&json!(0)),
        "timeline_ok": failing_features.is_empty(),
        "failing_features": failing_features,
    }))
}

fn materials(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let action = str_arg(a, "action").unwrap_or("list");
    match action {
        "list" => {
            let mats = exec(b, "material.list", json!({}))?;
            let apps = exec(b, "appearance.list", json!({}))?;
            Ok(json!({
                "materials": mats.get("materials").cloned().unwrap_or_else(|| json!([])),
                "appearances": apps.get("in_design").cloned().unwrap_or_else(|| json!([])),
            }))
        }
        "assign" => {
            let mut results = Map::new();
            if let Some(mat_name) = str_arg(a, "material") {
                let bodies = a.get("bodies").cloned().unwrap_or_else(|| json!([]));
                let res = exec(b, "material.assign", json!({"material": mat_name, "bodies": bodies}))?;
                results.insert("material".to_string(), res);
            }
            if a.contains_key("appearance") || a.contains_key("color") {
                let mut app_params = pick(a, &["appearance", "color", "opacity", "bodies"]);
                if let Some(faces) = a.get("faces") {
                    app_params["faces"] = faces.clone();
                }
                let res = exec(b, "appearance.assign", app_params)?;
                results.insert("appearance".to_string(), res);
            }
            if results.is_empty() {
                return Err("materials: `assign` requires `material`, `appearance`, or `color`".to_string());
            }
            Ok(Value::Object(results))
        }
        other => Err(format!("materials: unknown action `{other}`, must be `list` or `assign`")),
    }
}

fn artifacts(b: &mut dyn Backend, a: &Map<String, Value>) -> Result<Value, String> {
    let filter = str_arg(a, "filter").map(str::to_lowercase);
    let target_dir = match str_arg(a, "dir") {
        Some(d) => {
            solidcraft_engine::io::vfs::check(d).map_err(|e| format!("{d}: {e}"))?;
            std::path::PathBuf::from(d)
        }
        None => {
            if let Some(dp) = b.design_path() {
                let p = std::path::Path::new(&dp);
                let parent = p.parent().unwrap_or_else(|| std::path::Path::new("."));
                let p_str = parent.to_string_lossy();
                if solidcraft_engine::io::vfs::check(&p_str).is_ok() {
                    parent.to_path_buf()
                } else if let Some(scratch) = crate::scratch_dir() {
                    scratch
                } else {
                    return Ok(json!({"count": 0, "artifacts": []}));
                }
            } else if let Some(scratch) = crate::scratch_dir() {
                scratch
            } else if solidcraft_engine::io::vfs::check(".").is_ok() {
                std::path::PathBuf::from(".")
            } else {
                return Ok(json!({"count": 0, "artifacts": []}));
            }
        }
    };

    const CAD_EXTS: &[&str] = &["solidcraft", "step", "stp", "dxf", "stl", "3mf", "obj", "urdf", "png", "json"];
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&target_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            if !CAD_EXTS.contains(&ext.as_str()) {
                continue;
            }
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if let Some(ref f) = filter
                && !file_name.to_lowercase().contains(f)
                && !ext.contains(f)
            {
                continue;
            }
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());

            let kind = match ext.as_str() {
                "dxf" => "drawing",
                "png" => "image",
                "solidcraft" => "design",
                "step" | "stp" | "stl" | "3mf" | "obj" | "urdf" => "model",
                _ => "file",
            };

            found.push(json!({
                "name": file_name,
                "path": path.to_string_lossy().replace('\\', "/"),
                "extension": ext,
                "kind": kind,
                "size_bytes": size,
                "modified_epoch_s": modified,
            }));
        }
    }
    found.sort_by(|x, y| {
        let nx = x.get("name").and_then(Value::as_str).unwrap_or("");
        let ny = y.get("name").and_then(Value::as_str).unwrap_or("");
        nx.cmp(ny)
    });

    Ok(json!({
        "directory": target_dir.to_string_lossy().replace('\\', "/"),
        "count": found.len(),
        "artifacts": found,
    }))
}
