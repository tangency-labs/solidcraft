//! Script mode (scx, docs/mcp.md, "Script mode"): instead of the usual tools, the server lists `run`, `design`
//! and `look`, and the agent builds with short Python scripts in a CAD vocabulary (`box`,
//! `sketch(...).extrude`, `fillet`, `part`...). Each script runs in the sandbox ([`sandbox`]) and
//! every reply ends with what changed, so the agent checks its work without extra calls.
//! Measured against the usual tools on the same tasks, it cuts the cost of a modelling session by
//! 20-59 % (docs/mcp.md links the benchmark).

pub mod sandbox;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::Backend;
use crate::options::Options;

/// The server instructions in script mode: the vocabulary, as the agent sees it.
pub const INSTRUCTIONS: &str = r#"SolidCraft CAD. Units mm, Z up. Send Python scripts to `run` as {"code": "..."}; do the whole part in one script when you can; each line acts at once. Every reply ends with what changed: + new body, ~ changed body (volume before -> after, bbox), - removed. Check them; fix with another `run` (steps before an error stay). A bare last-line expression is printed.
op='new' makes a separate body; 'join'/'cut'/'intersect' act on the last body made or changed (into='Body' picks another). Name bodies (name=). Use loops for repeats.

Vocabulary (math, pi, sqrt, sin, cos ready):
  box(x, y, z, at=min corner | center=(..), op=, name=, into=)
  cylinder(h, d=|r=, at=base centre, axis='Z'|'X'|'Y', op=, name=)   cone(h, d1, d2=0, at=, axis=, op=)   sphere(d=|r=, at=, op=)
  hole(at=point ON a face, d, depth=None (through), direction=None (into the face))
  s = sketch('XY'|'XZ'|'YZ', offset=0)   # u,v = (x,y)|(x,z)|(y,z); offset along Z|Y|X
    s.rect(p0, p1) s.rect_center(c, w, h) s.circle(c, d=) s.slot(center, length, width, angle=0) s.polygon(c, sides, r_vertex) s.poly([pts])
    s.extrude(dist, op=, symmetric=False, name=)   # +dist along +Z|+Y|+X; circle in rect = hole
    s.revolve(axis='X'|'Y'|'Z', angle=360, op=)    # about a world axis
  loft([sketches], op=, name=)
  fillet([points on edges], r)   chamfer([points on edges], d)   # edge point: edges(body)[i]['mid']
  hollow(t, open='top'|'bottom'|'front'|...|None, body=)   # or shell([point on each open face], t)
  combine(target, [tools], op='cut'|'join'|'intersect')   move(bodies, by=(dx,dy,dz), rotate=('Z', deg), copy=False)   remove(bodies)  rename(body, new)
  with part('Name'): ...   # its features go in that part
  measure() -> {body: {volume, bbox, faces}}   edges(body) -> [{name, mid, length}]   faces(body) -> [{name, point}]
  cmd('command.id', **params) for anything else; commands('word') finds ids.
run(..., look=True) adds a picture.

Example:
  box(60, 40, 8, name='Plate')
  for x, y in [(8, 8), (52, 8), (8, 32), (52, 32)]:
      hole(at=(x, y, 8), d=6)
"#;

/// The tools a script reaches through `_call`: the ones the vocabulary is built on. Each is
/// answered exactly as when the agent calls it.
pub const HOST_TOOLS: &[&str] = &["execute", "measure", "list_commands", "new_design"];

/// Where scripts run.
#[derive(Clone, Debug, Default)]
pub enum Runner {
    /// In this process (tests; embedders that accept that a script exhausting memory ends the
    /// server).
    #[default]
    InProcess,
    /// In a `<program> scx-worker` child per script, capped in memory and time:
    /// `solidcraft-cli mcp` passes itself.
    Worker(PathBuf),
}

/// The tools of script mode.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "run",
            "title": "Run a build script",
            "description": "Run a Python build script in SolidCraft (vocabulary in the server instructions). Returns printed output, errors with line numbers and hints, and what changed (bodies, volumes, bboxes).",
            "inputSchema": {"type": "object", "properties": {
                "code": {"type": "string", "description": "Python script"},
                "look": {"type": "boolean", "description": "Add a small picture after it runs"},
                "new": {"type": ["boolean", "string"], "description": "true (or a name): start a new, empty design before the script runs"},
            }, "required": ["code"], "additionalProperties": false},
            "annotations": {"title": "Run a build script", "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true},
        }),
        json!({
            "name": "design",
            "title": "Outline the design",
            "description": "Outline of the design: every body with volume, bbox and faces, and the parts.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"title": "Outline the design", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "look",
            "title": "Look at the model",
            "description": "Small picture of the model.",
            "inputSchema": {"type": "object", "properties": {
                "view": {"type": "string", "description": "iso|front|top|right|back|left|bottom"},
                "size": {"type": "integer", "minimum": 64, "maximum": 1024},
            }, "additionalProperties": false},
            "annotations": {"title": "Look at the model", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
    ]
}

/// Runs a script-mode tool. Its arguments were checked against [`tool_definitions`].
pub fn call(b: &mut dyn Backend, name: &str, args: &Value, opts: Options, runner: &Runner) -> Value {
    let empty = Map::new();
    let a = args.as_object().unwrap_or(&empty);
    let job = match name {
        "run" => sandbox::Job::Run {
            code: a.get("code").and_then(Value::as_str).unwrap_or_default().to_owned(),
            fresh: a.get("new").cloned().unwrap_or(Value::Null),
        },
        "design" => sandbox::Job::Design,
        "look" => {
            let view = a.get("view").and_then(Value::as_str).unwrap_or("iso");
            let size = a.get("size").and_then(Value::as_u64).unwrap_or(384);
            let images = picture(b, view, size, opts);
            if images.is_empty() {
                return json!({"content": [{"type": "text", "text": "no picture"}], "isError": true});
            }
            return json!({"content": images});
        }
        _ => return json!({"content": [{"type": "text", "text": format!("unknown tool `{name}`")}], "isError": true}),
    };
    // The tools the script calls are the usual ones, shaped as usual.
    let inner = Options { scx: false, ..opts };
    let outcome = {
        let mut host = |tool: &str, args: &str| -> Result<String, String> {
            if !HOST_TOOLS.contains(&tool) {
                return Err(format!("`{tool}` is not one of the tools a script can call"));
            }
            let args: Value = serde_json::from_str(args).map_err(|e| format!("{tool}: arguments are not JSON ({e})"))?;
            let r = crate::tools::call_tool_with(b, tool, &args, inner);
            let text = r["content"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| c["type"] == "text")
                .and_then(|c| c["text"].as_str())
                .unwrap_or_default()
                .to_owned();
            if r["isError"] == true { Err(text) } else { Ok(text) }
        };
        match runner {
            Runner::InProcess => sandbox::execute(&job, &mut host),
            Runner::Worker(program) => sandbox::in_worker(program, &job, &mut host),
        }
    };
    let mut content = vec![json!({"type": "text", "text": outcome.text})];
    if name == "run" && !outcome.error && a.get("look").and_then(Value::as_bool).unwrap_or(false) {
        content.extend(picture(b, "iso", 384, inner));
    }
    let mut r = json!({"content": content});
    if outcome.error {
        r["isError"] = json!(true);
    }
    r
}

/// A small picture of the model, as image content blocks (none when it cannot be drawn).
fn picture(b: &mut dyn Backend, view: &str, size: u64, opts: Options) -> Vec<Value> {
    let size = size.clamp(64, 1024);
    let args = json!({"source": "model", "view": view, "width": size, "height": size * 3 / 4});
    let r = crate::tools::call_tool_with(b, "screenshot", &args, Options { scx: false, ..opts });
    r["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "image").cloned().collect()
}
