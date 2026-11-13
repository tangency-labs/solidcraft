//! Where MCP tool calls end up: a control-channel method call (`docs/control-protocol.md`).

#[cfg(not(target_arch = "wasm32"))]
mod remote;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::schema::ParamCheck;
use solidcraft_engine::view::Guides;
use solidcraft_viewport::StandardView;

/// Something that answers control-channel methods: `engine.execute`, `engine.commands`,
/// `document.inspect`, `ui.view`, `ui.render`, `ui.screenshot`, `control.running`, and
/// `control.pending` (the result of a request that was still running when its call returned:
/// `{pending: what | null, done?, reply?, lost?, running_s?}`).
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real window is attached (`ui.screenshot` can capture it).
    fn has_ui(&self) -> bool;
    fn describe(&self) -> String;
    /// The open design's file, when this side knows it (its folder joins the agent's folders,
    /// see `vfs::PathScope::with_design`). A running app adds its own, so `Remote` says nothing.
    fn design_path(&self) -> Option<String> {
        None
    }
    /// Commands run in this process, on this thread (the kernel's time in them can be measured
    /// here, #1833).
    fn in_process(&self) -> bool {
        false
    }
    /// A line every tool result carries (a session that is not what the user expects).
    fn notice(&self) -> Option<String> {
        None
    }
    /// How calls are checked against their commands' typed contracts (`--param-check`): by the
    /// headless session, or for `Remote` by the app or before the call is sent to it.
    fn param_check(&self) -> ParamCheck {
        ParamCheck::Off
    }
    /// The output options the app's Preferences ask for (its `mcp.options` reply), for a backend
    /// bridged to the app. `Ok(None)`: nothing to ask (no app, or one from before the setting), so
    /// the server's own options decide; `Err`: the app could not answer now (not running, busy),
    /// ask again on a later call.
    fn app_options(&mut self) -> Result<Option<Value>, String> {
        Ok(None)
    }
    /// A bridge to the same app on a connection of its own, for a backend bridged to one: the
    /// server answers `running` on it while this backend is busy with a call (#14007), as the
    /// app answers `control.running` on any connection at once. `None`: nothing to ask apart
    /// (an in-process session answers only between calls).
    #[cfg(not(target_arch = "wasm32"))]
    fn side_channel(&self) -> Option<Remote> {
        None
    }
}

/// How a tool error starts when a request is still running in the app after the wait: the
/// request was not cancelled and its reply waits on the connection; the `pending` tool gets it.
/// `batch` uses it to tell "still running" from "failed".
pub(crate) use solidcraft_control::STILL_RUNNING;

#[cfg(test)]
pub(crate) use remote::Waits;
#[cfg(not(target_arch = "wasm32"))]
pub use remote::{Auto, Remote};

/// How a tool error starts when no app listens at the bridged address. The agent reads it, and
/// `batch` uses it to tell "never sent" from "sent, reply lost".
pub(crate) const NOT_RUNNING: &str = "no SolidCraft app is listening at";

/// An in-process headless session (no window); renders with the CPU rasterizer.
pub struct Headless {
    pub session: Session,
    /// Orientation for renders; the camera is refitted to the model every time.
    pub view: StandardView,
    /// Application layout persists across calls, outside Session/document undo.
    pub panels: solidcraft_engine::panels::PanelResult<solidcraft_engine::panels::commands::InMemoryPanelBackend>,
}

impl Default for Headless {
    fn default() -> Self {
        Headless::new(Session::default())
    }
}

impl Headless {
    pub fn new(mut session: Session) -> Self {
        match solidcraft_engine::render::job::CpuHost::for_platform() {
            Ok(host) => {
                if let Err(e) = session.set_render_job_host(Some(host)) {
                    session.log.push(format!("image host unavailable: {e}"));
                }
            }
            Err(e) => session.log.push(format!("image host unavailable: {e}")),
        }
        Headless { session, view: StandardView::Iso, panels: solidcraft_engine::panels::commands::InMemoryPanelBackend::empty() }
    }
    pub fn with_panels(mut self, panels: solidcraft_engine::panels::commands::InMemoryPanelBackend) -> Self {
        self.panels = Ok(panels);
        self
    }
    fn execute_command(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        use solidcraft_engine::panels::commands;
        if commands::is_command(id) {
            self.panels.as_mut().map_err(|reason| commands::error_text(reason.clone()))?.execute(id, params).map_err(commands::error_text)
        } else {
            self.session.execute(id, params).map_err(|e| e.to_string())
        }
    }
    fn panel_sequence(&mut self, method: &str, p: &Value, steps: &[Value]) -> Result<Value, String> {
        use solidcraft_engine::panels::{Rule, commands, error};
        if steps.len() > 100_000 {
            return Err("script too long".into());
        }
        let rollback = p.get("rollback").and_then(Value::as_bool).unwrap_or(true);
        if method == "engine.batch"
            && let Some(refusal) = solidcraft_engine::batch_preflight::check(steps, self.session.param_check, &[])
        {
            return Ok(refusal.reply(steps.len(), rollback));
        }
        let panels = steps.iter().filter(|step| step_command(step).is_some_and(commands::is_command)).count();
        if method == "engine.batch" && rollback {
            if panels != steps.len() {
                return Err(commands::error_text(error(
                    Rule::Capability,
                    "atomic mixed document/layout batches are unavailable; use rollback:false or separate transactions",
                )));
            }
            return self.panels.as_mut().map_err(|reason| commands::error_text(reason.clone()))?.batch(steps).map_err(commands::error_text);
        }
        let mut results = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            let empty = json!({});
            let params = step.get("params").filter(|p| !p.is_null()).unwrap_or(&empty);
            let result = step_command(step).ok_or_else(|| "missing command".to_string()).and_then(|id| self.execute_command(id, params));
            match result {
                Ok(result) => results.push(result),
                Err(error) if method == "engine.batch" => return Ok(commands::sequential_failure(i, step_command(step), error, results)),
                Err(error) => return Err(error),
            }
        }
        if method == "engine.batch" { Ok(json!({"completed":results.len(),"results":results,"undone":0})) } else { Ok(json!(results)) }
    }
}

fn step_command(step: &Value) -> Option<&str> {
    step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)
}

fn dim(p: &Value, k: &str, default: f64) -> u32 {
    p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(default).clamp(16.0, 4096.0) as u32
}

impl Headless {
    fn call_at_current_level(&mut self, method: &str, p: Value) -> Result<Value, String> {
        if matches!(method, "engine.execute" | "command") && step_command(&p).is_some_and(solidcraft_engine::panels::commands::is_command) {
            let empty = json!({});
            return self.execute_command(step_command(&p).unwrap_or_default(), p.get("params").filter(|p| !p.is_null()).unwrap_or(&empty));
        }
        if matches!(method, "engine.script" | "engine.batch")
            && let Some(steps) = p
                .as_array()
                .or_else(|| p.get("commands").and_then(Value::as_array))
                .filter(|steps| steps.iter().any(|step| step_command(step).is_some_and(solidcraft_engine::panels::commands::is_command)))
        {
            return self.panel_sequence(method, &p, steps);
        }
        let s = &mut self.session;
        let str_p = |k: &str| p.get(k).and_then(Value::as_str);
        match method {
            "engine.execute" | "command" => {
                let id = str_p("command").ok_or("missing `command`")?;
                let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
                s.execute(id, &params).map_err(|e| e.to_string())
            }
            "engine.script" => s.run_script(&p).map(Value::from).map_err(|e| e.to_string()),
            "engine.commands" => s.execute("engine.commands", &json!({})).map_err(|e| e.to_string()),
            "document.inspect" => s.execute("document.inspect", &p).map_err(|e| e.to_string()),
            "ui.view" => {
                let v = str_p("view").unwrap_or("home");
                match v {
                    // Fit keeps the orientation, also of a camera `view.apply` chose.
                    "fit" => {
                        let b = solidcraft_engine::view::bounds(s).map_err(|e| e.to_string())?;
                        if let Some(c) = s.camera.as_mut() {
                            c.fit(&solidcraft_engine::render::view_bounds(&b));
                        }
                    }
                    _ => {
                        self.view = StandardView::parse(v).ok_or_else(|| format!("unknown view `{v}`"))?;
                        s.camera = None;
                    }
                }
                Ok(json!({"view": self.view}))
            }
            "ui.render" => {
                let (w, h) = (dim(&p, "width", 1280.0), dim(&p, "height", 800.0));
                // The camera `view.apply` chose, else the standard view framing the model.
                let cam = match s.camera {
                    Some(cam) => cam,
                    None => {
                        let mut cam = solidcraft_engine::view::home_camera(s).map_err(|e| e.to_string())?;
                        cam.set_view(self.view);
                        cam
                    }
                };
                // The grid and the axes are drawn unless the request leaves them out.
                let shown = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(true);
                let guides = Guides { grid: shown("grid"), axes: shown("axes") };
                let path = str_p("path").ok_or("missing `path`")?;
                // A path outside the agent's folders is refused before anything is drawn.
                solidcraft_engine::io::vfs::check(path).map_err(|e| format!("{path}: {e}"))?;
                let scene = solidcraft_engine::view::scene_with_guides(s, &cam, s.section, guides).map_err(|e| e.to_string())?;
                // Drawn on the GPU by the renderer the host installed (`solidcraft-cli mcp`).
                let png = solidcraft_engine::capture::png(&scene, &cam, w, h)?;
                solidcraft_engine::io::vfs::write(path, &png.bytes).map_err(|e| format!("{path}: {e}"))?;
                Ok(json!({
                    "path": path,
                    "bytes": png.bytes.len(),
                    "width": w,
                    "height": h,
                    "renderer": png.renderer,
                    "warning": solidcraft_engine::capture::warning(png.renderer),
                }))
            }
            "ui.screenshot" => Err("no window in headless mode; use ui.render".into()),
            // Calls run here one at a time: none is running while this one is asked, and nothing
            // is ever left running.
            "control.running" => Ok(json!({"running": []})),
            "control.pending" => Ok(json!({"pending": null})),
            "control.cancel" => Err("nothing can be cancelled in headless mode: each call runs to its end before it returns".into()),
            other => Err(format!("unknown method `{other}`")),
        }
    }
}
impl Backend for Headless {
    fn call(&mut self, method: &str, p: Value) -> Result<Value, String> {
        let own = self.session.param_check;
        if matches!(method, "engine.execute" | "command" | "engine.batch" | "engine.script")
            && let Some(value) = p.get("param_check")
        {
            self.session.param_check = value.as_str().and_then(ParamCheck::parse).ok_or("`param_check` must be off, warn or strict")?;
        }
        let reply = self.call_at_current_level(method, p);
        self.session.param_check = own;
        reply
    }
    fn has_ui(&self) -> bool {
        false
    }
    fn describe(&self) -> String {
        "headless in-process SolidCraft session".into()
    }
    fn design_path(&self) -> Option<String> {
        self.session.path.clone()
    }
    fn in_process(&self) -> bool {
        true
    }
    fn param_check(&self) -> ParamCheck {
        self.session.param_check
    }
}
