//! Install as an app (the browser build). A browser that can install the web app (a PWA: its own
//! window and launcher icon, working offline) offers it to the page; while it does, the File menu
//! has Install app…, which shows the browser's own install prompt. An installed app, or the app
//! running installed, gets no offer, so the item is gone. The host keeps the offer
//! (`Services::install_app`); desktop builds have none.
//!
//! Safari installs web apps too (Add to Home Screen on iPhone and iPad, Add to Dock on a Mac) but
//! offers no prompt a page can show. There the item opens a window with the steps.

use crate::SolveApp;
use crate::ui_kit;

/// How the browser installs the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Its own install prompt (`InstallApp::prompt`).
    Prompt,
    /// No prompt a page can show: SolidCraft shows these steps (Safari).
    Steps(Steps),
}

/// Safari's steps for installing a web app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steps {
    /// iPhone and iPad: Share ▸ Add to Home Screen.
    HomeScreen,
    /// A Mac (Safari 17 or later): File ▸ Add to Dock.
    Dock,
}

impl Steps {
    /// The steps' message ids, in order, and the one that says what follows.
    fn messages(self) -> (&'static [&'static str], &'static str) {
        match self {
            Steps::HomeScreen => (&["install.home_screen_1", "install.home_screen_2", "install.home_screen_3"], "install.home_screen_after"),
            Steps::Dock => (&["install.dock_1", "install.dock_2"], "install.dock_after"),
        }
    }
}

/// The browser's offer to install the app, kept by the host.
pub struct InstallApp {
    /// What the browser offers now; None: nothing (installed, or it can't install).
    pub offer: Box<dyn Fn() -> Option<Offer>>,
    /// Show the browser's install prompt (`Offer::Prompt`). Browsers only show it right after a
    /// click.
    pub prompt: Box<dyn Fn()>,
}

/// Safari's steps for this browser, from its `navigator.userAgent`, `navigator.vendor` and
/// `navigator.maxTouchPoints`; None: not Safari, or a Safari that can't install web apps.
/// Other iOS browsers are WebKit too (vendor Apple) but have no `Version/` and name themselves.
pub fn safari_steps(user_agent: &str, vendor: &str, touch_points: i32) -> Option<Steps> {
    let other = ["CriOS/", "FxiOS/", "EdgiOS/", "OPiOS/", "OPT/"];
    let safari = vendor.starts_with("Apple") && user_agent.contains("Safari/") && !other.iter().any(|o| user_agent.contains(o));
    if !safari {
        return None;
    }
    let version = user_agent.split("Version/").nth(1)?;
    // iPadOS Safari presents itself as a Mac; only its touch screen tells them apart.
    let mobile = ["iPhone", "iPad", "iPod"].iter().any(|d| user_agent.contains(d));
    if mobile || (user_agent.contains("Macintosh") && touch_points > 1) {
        return Some(Steps::HomeScreen);
    }
    let major: u32 = version.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    (user_agent.contains("Macintosh") && major >= 17).then_some(Steps::Dock)
}

/// What the browser offers now (None: nothing, so File has no Install app… item).
pub fn offered(app: &SolveApp) -> Option<Offer> {
    app.services.install_app.as_ref().and_then(|i| (i.offer)())
}

/// File ▸ Install app…: the browser's own prompt (browsers only show it right after a click), or
/// Safari's steps.
pub fn start(app: &mut SolveApp, offer: Offer) {
    match offer {
        Offer::Prompt => {
            if let Some(install) = &app.services.install_app {
                (install.prompt)();
            }
        }
        Offer::Steps(steps) => app.install_steps = Some(steps),
    }
}

/// The card with Safari's steps, while open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(steps) = app.install_steps else { return };
    let (lines, after) = steps.messages();
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_install_steps"), "web", crate::i18n::tr("install.title"))
        .actions("", crate::i18n::tr("install.done"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, crate::i18n::tr("install.steps_intro"), ui_kit::Text::Report("intro", false));
            for (i, id) in lines.iter().enumerate() {
                ui_kit::text(ui, &format!("{}. {}", i + 1, crate::i18n::tr(id)), ui_kit::Text::Report(&format!("step:{i}"), false));
            }
            ui_kit::text(ui, crate::i18n::tr(after), ui_kit::Text::Note("after"));
        });
    if closed || action != ui_kit::Action::None {
        app.install_steps = None;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use serde_json::json;

    use super::*;
    use crate::scenario::{Harness, handle_at};

    const ITEM: &str = "install.menu";

    fn host(h: &mut Harness, offer: Rc<Cell<Option<Offer>>>, prompts: Rc<Cell<u32>>) {
        h.app.services.install_app =
            Some(InstallApp { offer: Box::new(move || offer.get()), prompt: Box::new(move || prompts.set(prompts.get() + 1)) });
    }

    /// Open the File menu and look for Install app….
    fn file_menu_item(h: &mut Harness) -> bool {
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), true));
        h.frames(2);
        h.widget(crate::i18n::tr(ITEM)).is_some()
    }

    #[test]
    fn install_is_in_the_file_menu_only_while_offered_and_prompts_on_click() {
        let offer = Rc::new(Cell::new(None));
        let prompts = Rc::new(Cell::new(0));
        let mut h = Harness::new();
        host(&mut h, offer.clone(), prompts.clone());
        assert!(!file_menu_item(&mut h), "no offer, no menu item");

        offer.set(Some(Offer::Prompt));
        assert!(file_menu_item(&mut h), "offered: File has Install app…");
        assert_eq!(h.step(&json!({ "widget": crate::i18n::tr(ITEM) })), Ok(()));
        assert_eq!(prompts.get(), 1, "a click shows the browser's prompt");
        assert_eq!(h.app.install_steps, None, "no steps window for a browser with a prompt");

        offer.set(None);
        assert!(!file_menu_item(&mut h), "installed: the item is gone");
    }

    #[test]
    fn install_is_not_in_the_application_bar() {
        let mut h = Harness::new();
        host(&mut h, Rc::new(Cell::new(Some(Offer::Prompt))), Rc::new(Cell::new(0)));
        h.frames(2);
        assert!(handle_at("titlebar:install").is_none());
        assert!(h.widget(crate::i18n::tr(ITEM)).is_none(), "only in the File menu");
    }

    #[test]
    fn safari_shows_its_steps() {
        let prompts = Rc::new(Cell::new(0));
        let mut h = Harness::new();
        host(&mut h, Rc::new(Cell::new(Some(Offer::Steps(Steps::Dock)))), prompts.clone());
        assert!(file_menu_item(&mut h));
        assert_eq!(h.step(&json!({ "widget": crate::i18n::tr(ITEM) })), Ok(()));
        assert_eq!(h.app.install_steps, Some(Steps::Dock), "a click opens the steps");
        assert_eq!(prompts.get(), 0, "Safari has no prompt");
        h.frames(2);
        assert_eq!(h.step(&json!({ "widget": crate::i18n::tr("install.done") })), Ok(()));
        assert_eq!(h.app.install_steps, None, "Done closes the steps");
    }

    #[test]
    fn no_install_item_without_the_host_service() {
        let mut h = Harness::new();
        assert!(!file_menu_item(&mut h));
    }

    #[test]
    fn safari_steps_by_device() {
        const APPLE: &str = "Apple Computer, Inc.";
        let iphone =
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1";
        let mac =
            |v: &str| format!("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{v} Safari/605.1.15");
        assert_eq!(safari_steps(iphone, APPLE, 5), Some(Steps::HomeScreen));
        assert_eq!(safari_steps(&mac("18.5"), APPLE, 5), Some(Steps::HomeScreen), "an iPad presents itself as a Mac");
        assert_eq!(safari_steps(&mac("18.5"), APPLE, 0), Some(Steps::Dock));
        assert_eq!(safari_steps(&mac("17.0"), APPLE, 0), Some(Steps::Dock));
        assert_eq!(safari_steps(&mac("16.6"), APPLE, 0), None, "Add to Dock came with Safari 17");
        let chrome_ios = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/138.0.7204.156 Mobile/15E148 Safari/604.1";
        assert_eq!(safari_steps(chrome_ios, APPLE, 5), None, "Chrome on iOS isn't Safari");
        let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
        assert_eq!(safari_steps(chrome, "Google Inc.", 0), None, "Chrome prompts instead");
        let firefox = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:144.0) Gecko/20100101 Firefox/144.0";
        assert_eq!(safari_steps(firefox, "", 0), None);
        assert_eq!(safari_steps("", APPLE, 0), None, "hostile input is just not Safari");
        assert_eq!(safari_steps("Safari/ Version/99999999999999999999 Macintosh", APPLE, 0), None);
    }
}
