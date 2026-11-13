//! "Let AI agents connect" turned on and off while the app runs: the app's control channel (MCP)
//! opens on the port in Preferences > AI > Agents through MCP, moves when the port changes, and
//! closes when the setting is turned off. A port named at launch (`--control PORT`,
//! `SOLIDCRAFT_CONTROL_PORT`) wins for that run; the settings then wait for the next launch.
//!
//! The app (`apps/solidcraft`) owns the listener and its token file; this module only decides
//! when to start or stop it, through the [`Starter`] the app installs.

use std::sync::mpsc::Receiver;

use solidcraft_control::ControlRequest;
use solidcraft_control::auth::Listen;

use crate::SolveApp;

/// How long a changed setting stays put before the channel follows it, so typing or dragging a
/// port does not open a listener on every number on the way.
pub const SETTLE_S: f64 = 0.6;

/// The lowest port the setting takes: below it ports need administrator rights on most systems.
pub const MIN_PORT: u16 = 1024;

/// A channel the app started: its port, its requests, and how to close it again.
pub struct Started {
    pub port: u16,
    pub requests: Receiver<ControlRequest>,
    pub stop: Box<dyn FnOnce() + Send>,
}

/// Why the channel did not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// Another program (or another SolidCraft) listens on the port.
    PortTaken(u16),
    /// Anything else, as the system said it.
    Failed(String),
}

/// Starts the app's channel: on exactly a port, or on the default one or the next free one.
pub type Starter = Box<dyn FnMut(Listen) -> Result<Started, StartError>>;

/// What the settings panel shows about the channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nothing listens.
    Off,
    /// Listening on this port.
    Listening(u16),
    /// Started on this port at launch (`--control`): the settings apply at the next launch.
    FromLaunch(u16),
    /// The last start failed.
    Error(StartError),
}

#[derive(Default)]
pub struct ControlSwitch {
    starter: Option<Starter>,
    /// The port the launch named: it wins for this run.
    launch_port: Option<u16>,
    /// The channel this switch opened: its port and how to stop it.
    running: Option<(u16, Box<dyn FnOnce() + Send>)>,
    /// The setting last acted on (`None`: off); `None` before the launch.
    applied: Option<Option<u16>>,
    /// A changed setting and when it last changed, waiting [`SETTLE_S`].
    pending: Option<(Option<u16>, f64)>,
    error: Option<StartError>,
}

impl ControlSwitch {
    pub fn status(&self) -> Status {
        match (&self.error, self.launch_port, &self.running) {
            (Some(e), _, _) => Status::Error(e.clone()),
            (None, Some(port), _) => Status::FromLaunch(port),
            (None, None, Some((port, _))) => Status::Listening(*port),
            (None, None, None) => Status::Off,
        }
    }

    /// Stop the channel this switch opened (turned off or moved). At exit the app removes the
    /// token files of every channel it opened.
    fn stop(&mut self) {
        if let Some((_, stop)) = self.running.take() {
            stop();
        }
    }
}

/// The port the setting asks for, or `None` when the channel is off.
pub fn wanted(app: &SolveApp) -> Option<u16> {
    app.preferences.agent_control.then_some(app.preferences.agent_control_port.max(MIN_PORT))
}

/// The app's default port takes the next free one when it is taken, as it always has; any other
/// port is exactly that port, so the panel can say it is taken.
fn listen_on(port: u16) -> Listen {
    if port == solidcraft_engine::control_auth::APP.default_port { Listen::Default } else { Listen::Exact(port) }
}

/// At launch: install `starter` and open the channel the launch or the settings ask for.
/// `launch_port`: the port `--control`/`SOLIDCRAFT_CONTROL_PORT` named, which wins.
pub fn launch(app: &mut SolveApp, starter: Starter, launch_port: Option<u16>) {
    let switch = &mut app.control_switch;
    switch.starter = Some(starter);
    switch.launch_port = launch_port;
    match launch_port {
        Some(port) => start(app, Listen::Exact(port)),
        None => {
            let want = wanted(app);
            app.control_switch.applied = Some(want);
            if let Some(port) = want {
                start(app, listen_on(port));
            }
        }
    }
}

fn start(app: &mut SolveApp, listen: Listen) {
    let Some(starter) = app.control_switch.starter.as_mut() else {
        return;
    };
    match starter(listen) {
        Ok(started) => {
            app.control_switch.error = None;
            if app.control_switch.launch_port.is_none() {
                app.control_switch.running = Some((started.port, started.stop));
            }
            app.control_rx = Some(started.requests);
        }
        Err(e) => app.control_switch.error = Some(e),
    }
}

/// Every frame: once a changed setting has settled, close the channel and open it where the
/// setting now asks. Nothing happens without a starter (tests, the web app) or when the launch
/// named a port.
pub fn tick(app: &mut SolveApp, now: f64, ctx: Option<&egui::Context>) {
    let switch = &app.control_switch;
    if switch.starter.is_none() || switch.launch_port.is_some() {
        return;
    }
    let (applied, pending) = (switch.applied, switch.pending);
    let want = wanted(app);
    if applied == Some(want) {
        app.control_switch.pending = None;
        return;
    }
    let since = match pending {
        Some((pending, since)) if pending == want => since,
        _ => {
            app.control_switch.pending = Some((want, now));
            now
        }
    };
    if now - since < SETTLE_S {
        if let Some(ctx) = ctx {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(SETTLE_S - (now - since)));
        }
        return;
    }
    app.control_switch.pending = None;
    app.control_switch.applied = Some(want);
    app.control_switch.stop();
    app.control_switch.error = None;
    app.control_rx = None;
    if let Some(port) = want {
        start(app, listen_on(port));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};

    /// What the fake app was asked to do: "start Exact(7999)", "stop 7999".
    type Log = Arc<Mutex<Vec<String>>>;

    /// A starter that records each start and stop; `taken` ports fail as in use.
    fn starter(log: &Log, taken: &'static [u16]) -> Starter {
        let log = Arc::clone(log);
        Box::new(move |listen| {
            let port = match listen {
                Listen::Exact(p) => p,
                Listen::Default => solidcraft_engine::control_auth::APP.default_port,
            };
            log.lock().unwrap().push(format!("start {listen:?}"));
            if taken.contains(&port) {
                return Err(StartError::PortTaken(port));
            }
            let (_tx, requests) = channel();
            let log = Arc::clone(&log);
            Ok(Started { port, requests, stop: Box::new(move || log.lock().unwrap().push(format!("stop {port}"))) })
        })
    }

    fn take(log: &Log) -> Vec<String> {
        std::mem::take(&mut *log.lock().unwrap())
    }

    /// Turned on, the channel opens on the app's default port; turning it off closes the channel
    /// once the change settles, turning it on opens it again, a new port moves it, and nothing
    /// restarts.
    #[test]
    fn the_setting_starts_and_stops_the_channel_without_a_restart() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        assert!(!app.preferences.agent_control, "off until the user turns it on");
        app.preferences.agent_control = true;
        assert_eq!(app.preferences.agent_control_port, solidcraft_engine::control_auth::APP.default_port);
        let log = Log::default();
        launch(&mut app, starter(&log, &[]), None);
        assert_eq!(take(&log), ["start Default"]);
        assert_eq!(app.control_switch.status(), Status::Listening(27878));
        assert!(app.control_rx.is_some());

        app.preferences.agent_control = false;
        tick(&mut app, 10.0, None);
        tick(&mut app, 10.3, None);
        assert!(take(&log).is_empty(), "waits for the change to settle");
        tick(&mut app, 10.0 + SETTLE_S + 0.01, None);
        assert_eq!(take(&log), ["stop 27878"]);
        assert_eq!(app.control_switch.status(), Status::Off);
        assert!(app.control_rx.is_none());
        tick(&mut app, 20.0, None);
        assert!(take(&log).is_empty(), "acts once");

        app.preferences.agent_control_port = 7999;
        app.preferences.agent_control = true;
        tick(&mut app, 30.0, None);
        tick(&mut app, 31.0, None);
        assert_eq!(take(&log), ["start Exact(7999)"]);
        assert_eq!(app.control_switch.status(), Status::Listening(7999));

        // Dragging through ports only opens the last one.
        for (i, port) in [8000, 8001, 8002].into_iter().enumerate() {
            app.preferences.agent_control_port = port;
            tick(&mut app, 40.0 + i as f64 * 0.1, None);
        }
        tick(&mut app, 41.0, None);
        assert_eq!(take(&log), ["stop 7999", "start Exact(8002)"]);
    }

    /// Off in the settings: nothing listens at launch; a port below 1024 is raised to 1024.
    #[test]
    fn off_at_launch_opens_nothing() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.agent_control = false;
        let log = Log::default();
        launch(&mut app, starter(&log, &[]), None);
        assert!(take(&log).is_empty());
        assert_eq!(app.control_switch.status(), Status::Off);
        app.preferences.agent_control = true;
        app.preferences.agent_control_port = 80;
        tick(&mut app, 0.0, None);
        tick(&mut app, 1.0, None);
        assert_eq!(take(&log), ["start Exact(1024)"]);
    }

    /// A taken port says so in the panel, and is not tried again every frame; another port
    /// clears it.
    #[test]
    fn a_taken_port_is_reported() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.agent_control = true;
        app.preferences.agent_control_port = 7000;
        let log = Log::default();
        launch(&mut app, starter(&log, &[7000]), None);
        assert_eq!(take(&log), ["start Exact(7000)"]);
        assert_eq!(app.control_switch.status(), Status::Error(StartError::PortTaken(7000)));
        tick(&mut app, 5.0, None);
        assert!(take(&log).is_empty());
        app.preferences.agent_control_port = 7001;
        tick(&mut app, 6.0, None);
        tick(&mut app, 7.0, None);
        assert_eq!(take(&log), ["start Exact(7001)"]);
        assert_eq!(app.control_switch.status(), Status::Listening(7001));
    }

    /// `--control PORT` wins for the run: it opens that port whatever the settings say, and
    /// changing the settings waits for the next launch.
    #[test]
    fn a_port_named_at_launch_wins_for_the_run() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.preferences.agent_control = false;
        let log = Log::default();
        launch(&mut app, starter(&log, &[]), Some(7878));
        assert_eq!(take(&log), ["start Exact(7878)"]);
        assert_eq!(app.control_switch.status(), Status::FromLaunch(7878));
        assert!(app.control_rx.is_some());
        app.preferences.agent_control = true;
        app.preferences.agent_control_port = 9000;
        tick(&mut app, 0.0, None);
        tick(&mut app, 5.0, None);
        assert!(take(&log).is_empty());
    }
}
