//! What the Browser shows on the row of a component linked to another design file. Whether a
//! file is there and still the one this design read takes reading it, so it is asked beside the
//! window: when the design's links change, and when the window comes back to the front.

use crate::SolveApp;
use crate::ui_kit::IconBadge;
use egui::Color32;
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::DesignLink;
use solidcraft_theme::{Tokens, contrast_ratio};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

type Reply = Result<Value, String>;

/// What the badge on a linked component's icon says about its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkState {
    /// The file is there and is the one this design last read.
    Linked,
    /// The file is there but is not the one this design last read.
    Changed,
    /// The file is not there.
    Missing,
    /// Nobody looked at the file; it may be fine.
    Unchecked,
}

impl LinkState {
    /// The glyph on the badge: a chain, broken for a file that is not there.
    fn glyph(self) -> &'static str {
        match self {
            LinkState::Missing => "link_broken",
            _ => "link",
        }
    }

    /// The badge as the Browser draws it, in this theme's colours. A state nobody checked is only
    /// outlined, because it says nothing about the file.
    fn badge(self) -> IconBadge<'static> {
        let t = Tokens::get();
        let (fill, ink, outlined) = match self {
            LinkState::Linked => (t.selected_bg, t.accent, false),
            LinkState::Changed => (t.warning, ink_on(t.warning), false),
            LinkState::Missing => (t.error, ink_on(t.error), false),
            LinkState::Unchecked => (t.panel, t.text_dim, true),
        };
        IconBadge { glyph: self.glyph(), fill, ink, outlined }
    }
}

/// Text ink for a solid badge fill: whichever of the theme's own on-fill colours reads best on it.
fn ink_on(fill: Color32) -> Color32 {
    let t = Tokens::get();
    [t.on_accent, t.tab_on_fill_dark, t.form_ink]
        .into_iter()
        .fold(t.text, |best, ink| if contrast_ratio(ink, fill) > contrast_ratio(best, fill) { ink } else { best })
}

/// The row of one linked component: its badge, and what hovering it says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LinkBadge {
    pub(crate) state: LinkState,
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

    /// The badge on the component icon at the start of its row.
    pub(crate) fn icon_badge(&self) -> IconBadge<'static> {
        self.state.badge()
    }
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
    let mut copy = app.session.scratch();
    // A scratch copy has no file: its links are read against this session's.
    copy.path.clone_from(&app.session.path);
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

/// A missing or changed file gets the badge that says so. A file that was not looked at may be
/// fine, so it gets the outlined badge and a note saying why.
pub(crate) fn badge_of(link: &Value) -> Option<(u64, LinkBadge)> {
    let component = link["component"].as_u64()?;
    let file = link["file"].as_str()?.to_string();
    let (state, note) = match (link["found"].as_bool(), link["changed"].as_bool()) {
        (Some(false), _) => (LinkState::Missing, Some("ui.link_missing")),
        (Some(true), Some(true)) => (LinkState::Changed, Some("ui.link_changed")),
        (Some(true), _) => (LinkState::Linked, None),
        (None, _) if link.get("host").is_some() => (LinkState::Unchecked, Some("ui.link_not_checked_network")),
        (None, _) => (LinkState::Unchecked, Some("ui.link_not_checked")),
    };
    Some((component, LinkBadge { state, note, file }))
}
