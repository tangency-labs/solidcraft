//! Support and community links: the Donate and Discord pills in the title bar before Report a bug, the
//! Help menu entries, the About window's buttons and the start page's links all come from [`LINKS`],
//! so a URL or a label is written once. Clicking one opens its page in the system browser; nothing
//! here opens a page by itself.
//!
//! The title bar's pills follow two preferences (Preferences › General › Community, both on by
//! default). The small × inside a pill turns its preference off. The Help menu, About and the start
//! page keep their links whatever those preferences say.

use crate::SolveApp;
use crate::prefs::Prefs;

pub const KOFI_URL: &str = "https://ko-fi.com/bherbruck";
pub const DISCORD_URL: &str = "https://discord.gg/FtU4NQNVDH";

/// One link and every place that shows it.
pub struct Link {
    /// The id the Help menu and the start page's links go by.
    pub id: &'static str,
    pub url: &'static str,
    /// The icon master (`crates/look/icons/design/svg/flat`): the service's brand mark.
    pub icon: &'static str,
    /// Message ids: the Help menu entry, the status bar button's short text, the longer text of the
    /// About and start page buttons, the tooltip and the status bar's hide button tooltip.
    pub menu: &'static str,
    pub button: &'static str,
    pub label: &'static str,
    pub tip: &'static str,
    pub hide_tip: &'static str,
    /// Message ids of the accessible names of the pill and of its ×.
    pub name: &'static str,
    pub hide_name: &'static str,
    /// Whether the title bar shows this link's pill, whether it carries its name on a wide bar, and
    /// the switch that stops it.
    pub named: bool,
    pub fill: egui::Color32,
    pub shown: fn(&Prefs) -> bool,
    pub hide: fn(&mut Prefs),
}

pub const LINKS: &[Link] = &[
    Link {
        id: "kofi",
        url: KOFI_URL,
        icon: "kofi",
        menu: "links.kofi_menu",
        button: "links.kofi_button",
        label: "links.kofi_label",
        tip: "links.kofi_tip",
        hide_tip: "links.kofi_hide",
        name: "links.kofi_name",
        hide_name: "links.kofi_hide_name",
        named: true,
        fill: solidcraft_ui_kit::app_bar::brand::KOFI,
        shown: |p| p.show_donate_button,
        hide: |p| p.show_donate_button = false,
    },
    Link {
        id: "discord",
        url: DISCORD_URL,
        icon: "discord",
        menu: "links.discord_menu",
        button: "links.discord_button",
        label: "links.discord_label",
        tip: "links.discord_tip",
        hide_tip: "links.discord_hide",
        name: "links.discord_name",
        hide_name: "links.discord_hide_name",
        named: false,
        fill: solidcraft_ui_kit::app_bar::brand::DISCORD,
        shown: |p| p.show_discord_button,
        hide: |p| p.show_discord_button = false,
    },
];

pub fn by_id(id: &str) -> Option<&'static Link> {
    LINKS.iter().find(|l| l.id == id)
}

/// Open `url` in the system browser, as the Help menu's Documentation does.
pub fn open(ctx: &egui::Context, url: &str) {
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// Open the link `id`; false for an id that is not in [`LINKS`].
pub fn run(ctx: &egui::Context, id: &str) -> bool {
    by_id(id).map(|l| open(ctx, l.url)).is_some()
}

/// The links whose title bar pill is on.
pub fn title_bar(app: &SolveApp) -> impl Iterator<Item = &'static Link> + '_ {
    LINKS.iter().filter(|l| (l.shown)(&app.preferences))
}

/// The title bar's pills, left to right; `wide` gives the named ones their name.
pub fn pills(app: &SolveApp, wide: bool) -> Vec<solidcraft_ui_kit::app_bar::Pill<'static>> {
    title_bar(app)
        .map(|l| solidcraft_ui_kit::app_bar::Pill {
            id: l.id,
            icon: l.icon,
            label: if l.named { solidcraft_ui_text::tr(l.button) } else { "" },
            labelled: l.named && wide,
            tip: solidcraft_ui_text::tr(l.tip),
            hide_tip: solidcraft_ui_text::tr(l.hide_tip),
            name: solidcraft_ui_text::tr(l.name),
            hide_name: solidcraft_ui_text::tr(l.hide_name),
            fill: l.fill,
            ink: egui::Color32::WHITE,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_link_opens_its_own_page_and_only_that() {
        crate::i18n::install();
        let ctx = egui::Context::default();
        let urls: Vec<_> = LINKS.iter().map(|l| l.url).collect();
        assert_eq!(urls, ["https://ko-fi.com/bherbruck", "https://discord.gg/FtU4NQNVDH"]);
        for link in LINKS {
            ctx.begin_pass(egui::RawInput::default());
            assert!(run(&ctx, link.id), "{}", link.id);
            let mut out = ctx.end_pass();
            out.textures_delta.clear();
            let opened = out.platform_output.commands.iter().find_map(|c| match c {
                egui::OutputCommand::OpenUrl(u) => Some(u.url.clone()),
                _ => None,
            });
            assert_eq!(opened.as_deref(), Some(link.url), "{}", link.id);
        }
        ctx.begin_pass(egui::RawInput::default());
        assert!(!run(&ctx, "nothing"));
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        assert!(out.platform_output.commands.iter().all(|c| !matches!(c, egui::OutputCommand::OpenUrl(_))));
    }

    #[test]
    fn every_link_has_text_and_an_icon_in_the_catalog() {
        crate::i18n::install();
        for link in LINKS {
            for id in [link.menu, link.button, link.label, link.tip, link.hide_tip, link.name, link.hide_name] {
                assert_ne!(solidcraft_ui_text::tr(id), id, "{id} has no English text");
            }
            assert!(solidcraft_look::icons::SET.has_master(link.icon), "{}", link.icon);
        }
    }

    #[test]
    fn both_buttons_are_on_by_default_and_the_switches_hide_only_their_own() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        assert_eq!(title_bar(&app).map(|l| l.id).collect::<Vec<_>>(), ["kofi", "discord"]);
        (LINKS[0].hide)(&mut app.preferences);
        assert_eq!(title_bar(&app).map(|l| l.id).collect::<Vec<_>>(), ["discord"]);
        (LINKS[1].hide)(&mut app.preferences);
        assert_eq!(title_bar(&app).count(), 0);
        // The links themselves stay: the Help menu, About and the start page list all of LINKS.
        assert_eq!(LINKS.len(), 2);
    }
}
