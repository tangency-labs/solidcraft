//! What the Browser shows on the row of a component linked to another design file. Whether a
//! file is there and still the one this design read takes reading it, so it is asked beside the
//! window: when the design's links change, and when the window comes back to the front.

use crate::SolveApp;
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::DesignLink;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

type Reply = Result<Value, String>;

/// The row of one linked component: its badge, and what hovering it says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LinkBadge {
    pub(crate) badge: &'static str,
    /// The message id of what is wrong or unknown about the file.
    pub(crate) note: Option<&'static str>,
    pub(crate) file: String,
}

impl LinkBadge {
    pub(crate) fn tooltip(&self) -> String {
        let source = format!("{} {}", solidcraft_ui_text::tr("ui.link_source"), self.file);
        match self.note {
            Some(note) => format!("{source}\n{}", solidcraft_ui_text::tr(note)),
            None => source,
        }
    }
}

/// The badges of a row that may be pinned and may be linked.
pub(crate) fn with_link(grounded: bool, link: Option<&LinkBadge>) -> Vec<&'static str> {
    grounded.then_some("pin").into_iter().chain(link.map(|l| l.badge)).collect()
}

/// The last answer, and the question it answers.
#[derive(Default)]
pub struct LinkBadges {
    asked: Option<(Option<String>, BTreeMap<u64, DesignLink>)>,
    job: Option<Receiver<Reply>>,
    shown: HashMap<u64, LinkBadge>,
    /// The window came back to the front: the files may be different now.
    again: bool,
}

impl LinkBadges {
    pub(crate) fn get(&self, component: u64) -> Option<&LinkBadge> {
        self.shown.get(&component)
    }

    fn is_current(&self, session: &Session) -> bool {
        self.asked.as_ref().is_some_and(|(path, links)| *path == session.path && *links == session.doc.links)
    }

    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// Once a frame: take a finished answer, and ask again if the question has changed. Asks nothing
/// while the design has no links, and reads no file here.
pub(crate) fn refresh(app: &mut SolveApp, ctx: &egui::Context) {
    if app.session.doc.links.is_empty() {
        if app.tree.links.asked.is_some() {
            app.tree.links = LinkBadges::default();
        }
        return;
    }
    app.tree.links.again |= ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
    if let Some(job) = &app.tree.links.job {
        match job.try_recv() {
            Ok(reply) => {
                app.tree.links.job = None;
                if let (true, Ok(reply)) = (app.tree.links.is_current(&app.session), reply) {
                    app.tree.links.shown = badges_of(&reply);
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => app.tree.links.job = None,
        }
    }
    let current = app.tree.links.is_current(&app.session);
    if current && !app.tree.links.again {
        return;
    }
    let copy = app.session.scratch();
    let badges = &mut app.tree.links;
    badges.again = false;
    if !current {
        badges.shown.clear();
        badges.asked = Some((copy.path.clone(), copy.doc.links.clone()));
    }
    badges.job = ask_beside(copy);
    ctx.request_repaint_after(Duration::from_millis(100));
}

#[cfg(not(target_arch = "wasm32"))]
fn ask_beside(mut copy: Session) -> Option<Receiver<Reply>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("link-badges".into())
        .spawn(move || {
            let _ = tx.send(copy.execute("component.links", &json!({})).map_err(|e| e.to_string()));
        })
        .ok()?;
    Some(rx)
}

/// No threads on the web, where the files are in memory.
#[cfg(target_arch = "wasm32")]
fn ask_beside(mut copy: Session) -> Option<Receiver<Reply>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = tx.send(copy.execute("component.links", &json!({})).map_err(|e| e.to_string()));
    Some(rx)
}

fn badges_of(reply: &Value) -> HashMap<u64, LinkBadge> {
    reply["links"].as_array().into_iter().flatten().filter_map(badge_of).collect()
}

/// A warning means something is wrong with the file. A file that was not looked at may be fine,
/// so it gets the neutral badge and a note saying why.
pub(crate) fn badge_of(link: &Value) -> Option<(u64, LinkBadge)> {
    let component = link["component"].as_u64()?;
    let file = link["file"].as_str()?.to_string();
    let (badge, note) = match (link["found"].as_bool(), link["changed"].as_bool()) {
        (Some(false), _) => ("link_warning", Some("ui.link_missing")),
        (Some(true), Some(true)) => ("link_warning", Some("ui.link_changed")),
        (Some(true), _) => ("link", None),
        (None, _) if link.get("host").is_some() => ("link_unchecked", Some("ui.link_not_checked_network")),
        (None, _) => ("link_unchecked", Some("ui.link_not_checked")),
    };
    Some((component, LinkBadge { badge, note, file }))
}
