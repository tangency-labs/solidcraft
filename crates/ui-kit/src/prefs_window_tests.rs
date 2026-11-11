use super::*;
use crate::test_support::{key, pointer};
use solidcraft_ui_registry as registry;

#[derive(Clone, Debug, Default, PartialEq)]
struct Values {
    grid: bool,
    shadow: bool,
}

#[derive(Default)]
struct Fake {
    values: Values,
    window: State<Values>,
    drawn: Vec<&'static str>,
    left: usize,
    closed: usize,
    /// Restores that fail before one succeeds.
    failing_restores: usize,
    /// Applies that fail before one succeeds.
    failing_applies: usize,
    failing_confirmations: usize,
}

impl Host for Fake {
    type Snapshot = Values;
    fn window(&mut self) -> &mut State<Values> {
        &mut self.window
    }
    fn window_ref(&self) -> &State<Values> {
        &self.window
    }
    fn take_snapshot(&self) -> Values {
        self.values.clone()
    }
    fn matches(&self, snapshot: &Values) -> bool {
        *snapshot == self.values
    }
    fn restore(&mut self, snapshot: &Values) -> Result<(), String> {
        if self.failing_restores > 0 {
            self.failing_restores -= 1;
            return Err("not now".into());
        }
        self.values = snapshot.clone();
        Ok(())
    }
    fn confirm(&mut self) -> Result<(), String> {
        if self.failing_confirmations > 0 {
            self.failing_confirmations -= 1;
            return Err("invalid field".into());
        }
        Ok(())
    }
    fn apply(&mut self) -> Result<(), String> {
        if self.failing_applies > 0 {
            self.failing_applies -= 1;
            return Err("no room".into());
        }
        Ok(())
    }
    fn page_left(&mut self) {
        self.left += 1;
    }
    fn closed(&mut self) {
        self.closed += 1;
    }
}

const PAGES: &[Page<Fake>] = &[
    Page::Host(HostPage {
        icon: Icon::Kit("settings"),
        label: "ui.general",
        group: None,
        handle: "test:page:alpha",
        scroll: true,
        draw: |ui, fake| {
            fake.drawn.push("alpha");
            crate::settings_page(ui, |ui| {
                crate::checkbox(ui, "Show grid", &mut fake.values.grid);
            });
        },
        defaults: |fake| fake.values.grid = false,
    }),
    Page::Host(HostPage {
        icon: Icon::Kit("orbit"),
        label: "ui.navigation",
        group: Some("prefs.input"),
        handle: "test:page:beta",
        scroll: true,
        draw: |ui, fake| {
            fake.drawn.push("beta");
            crate::settings_page(ui, |ui| {
                crate::checkbox(ui, "Ground shadow", &mut fake.values.shadow);
            });
        },
        defaults: |fake| fake.values.shadow = false,
    }),
];

fn options(prefix: &'static str) -> Options<'static, Fake> {
    Options {
        id: egui::Id::unique(("test_prefs_window", prefix)),
        prefix,
        labels: Labels {
            title: "Preferences",
            icon: "settings",
            ok: "OK",
            cancel: "Cancel",
            apply: "Apply",
            search: "Search",
            no_results: "Nothing matches",
            restore_defaults: "Restore Defaults",
            restore_defaults_hint: "Reset this page",
        },
        sidebar_footer: None,
        above_page: None,
    }
}

fn frame(ctx: &egui::Context, fake: &mut Fake, events: Vec<egui::Event>) -> Option<Button> {
    frame_as(ctx, fake, events, "test")
}

fn frame_as(ctx: &egui::Context, fake: &mut Fake, events: Vec<egui::Event>, prefix: &'static str) -> Option<Button> {
    registry::clear_handles();
    fake.drawn.clear();
    let mut pressed = None;
    crate::test_support::raw_frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        },
        |ui| pressed = show(ui.ctx(), fake, PAGES, &options(prefix), |_| {}),
    );
    pressed
}

fn click(ctx: &egui::Context, fake: &mut Fake, handle: &str) -> Option<Button> {
    let at = registry::rect_of(handle).map(|rect| rect.center()).or_else(|| registry::handle_at(handle));
    let at = at.unwrap_or_else(|| panic!("{handle} is published"));
    frame(ctx, fake, pointer(at, true)).or(frame(ctx, fake, pointer(at, false)))
}

fn opened() -> (egui::Context, Fake) {
    let ctx = crate::test_support::context();
    let mut fake = Fake::default();
    fake.window.open_at(0, PAGES.len());
    for _ in 0..3 {
        frame(&ctx, &mut fake, Vec::new());
    }
    (ctx, fake)
}

#[test]
fn the_window_switches_page_searches_and_highlights_and_cancel_restores_the_snapshot() {
    let (ctx, mut fake) = opened();
    assert!(registry::rect_of("test:page:alpha").is_some() && registry::rect_of("test:page:beta").is_some(), "both tabs are listed");
    assert_eq!(fake.drawn, ["alpha"], "only the selected page draws");
    assert_eq!(fake.window.snapshot(), Some(&Values::default()), "opening takes the snapshot");

    assert_eq!(click(&ctx, &mut fake, "test:page:beta"), None);
    assert_eq!((fake.window.section, fake.left), (1, 1), "a tab shows its page and leaves the other");
    frame(&ctx, &mut fake, Vec::new());
    assert_eq!(fake.drawn, ["beta"]);

    frame(&ctx, &mut fake, vec![egui::Event::Text("grid".into())]);
    for _ in 0..2 {
        frame(&ctx, &mut fake, Vec::new());
    }
    assert_eq!(fake.window.filter, "grid", "typing goes to the search field");
    let (query, _, hits) = settings_search::nav(&ctx);
    assert_eq!(query, "grid");
    assert_eq!(hits.iter().map(|hit| (hit.page, hit.label.as_str())).collect::<Vec<_>>(), [(0, "Show grid")], "the match on the other page");
    assert!(registry::handle_at("test:search:current").is_some(), "the current match is outlined");

    frame(&ctx, &mut fake, key(egui::Key::Enter));
    assert_eq!((fake.window.section, fake.window.filter.as_str()), (0, ""), "Enter opens the match's page and leaves search");
    frame(&ctx, &mut fake, Vec::new());
    assert_eq!(fake.drawn, ["alpha"]);
    assert!(registry::handle_at("test:revealed").is_some(), "the opened row is outlined");

    fake.values.grid = true;
    frame(&ctx, &mut fake, Vec::new());
    assert!(apply_pending(&fake), "a change since opening is something to apply");
    assert_eq!(click(&ctx, &mut fake, "test:cancel"), Some(Button::Cancel));
    assert_eq!(fake.values, Values::default(), "Cancel puts the snapshot back");
    assert!(!fake.window.open && fake.window.snapshot().is_none());
    assert_eq!(fake.closed, 1);
}

#[test]
fn apply_keeps_the_change_and_restore_defaults_resets_only_the_page_shown() {
    let (ctx, mut fake) = opened();
    fake.values = Values { grid: true, shadow: true };
    frame(&ctx, &mut fake, Vec::new());
    assert_eq!(click(&ctx, &mut fake, "test:apply"), Some(Button::Apply));
    assert!(!apply_pending(&fake) && fake.window.open, "Apply keeps the window open with nothing left to apply");
    assert_eq!(click(&ctx, &mut fake, "test:defaults"), Some(Button::Defaults));
    assert_eq!(fake.values, Values { grid: false, shadow: true }, "General's defaults leave the other page alone");
    assert_eq!(click(&ctx, &mut fake, "test:cancel"), Some(Button::Cancel));
    assert_eq!(fake.values, Values { grid: true, shadow: true }, "Cancel returns to the last Apply");
}

#[test]
fn a_failed_restore_keeps_the_window_and_the_snapshot_until_one_succeeds() {
    let (ctx, mut fake) = opened();
    fake.values.shadow = true;
    fake.failing_restores = 1;
    frame(&ctx, &mut fake, Vec::new());
    assert_eq!(click(&ctx, &mut fake, "test:cancel"), Some(Button::Cancel));
    assert!(fake.window.open && fake.window.snapshot().is_some() && fake.closed == 0);
    assert_eq!(fake.window.restore_error(), Some("not now"));
    press(&mut fake, PAGES, Button::Apply).unwrap();
    assert!(apply_pending(&fake), "Apply waits for the restore");
    press(&mut fake, PAGES, Button::Ok).unwrap();
    assert_eq!(fake.values, Values::default(), "after a failed restore, OK reverts too");
    assert!(!fake.window.open && fake.window.restore_error().is_none() && fake.closed == 1);
    frame(&ctx, &mut fake, Vec::new());
    assert!(fake.drawn.is_empty() && registry::rect_of("prefs:window").is_none(), "a closed window draws nothing");
}

#[test]
fn a_failed_apply_keeps_the_last_snapshot_and_the_footer_shows_it_until_a_close_reverts() {
    let (ctx, mut fake) = opened();
    fake.values.grid = true;
    fake.failing_applies = 1;
    assert_eq!(press(&mut fake, PAGES, Button::Apply), Err("no room".to_owned()), "a command gets the reason");
    assert_eq!(fake.window.snapshot(), Some(&Values::default()), "the last snapshot stays");
    assert!(fake.window.restore_error().is_none() && apply_pending(&fake), "a command's failure is only reported");

    fake.failing_applies = 1;
    frame(&ctx, &mut fake, Vec::new());
    assert_eq!(click(&ctx, &mut fake, "test:apply"), Some(Button::Apply));
    assert_eq!(fake.window.restore_error(), Some("no room"), "the footer's failure shows on the window");
    assert_eq!(fake.window.snapshot(), Some(&Values::default()));
    press(&mut fake, PAGES, Button::Apply).unwrap();
    assert_eq!(fake.window.snapshot(), Some(&Values::default()), "Apply waits while the failure shows");
    press(&mut fake, PAGES, Button::Ok).unwrap();
    assert_eq!(fake.values, Values::default(), "after a failed Apply, a close reverts");
    assert!(!fake.window.open && fake.window.restore_error().is_none() && fake.closed == 1);
}

#[test]
fn a_hostile_page_index_shows_the_last_page_and_no_pages_draws_no_body() {
    let mut fake = Fake::default();
    fake.window.open_at(usize::MAX, PAGES.len());
    assert_eq!(fake.window.section, 1);
    fake.window.open_at(3, 0);
    assert_eq!(fake.window.section, 0);
    press(&mut fake, &[], Button::Defaults).unwrap();
    assert_eq!(fake.values, Values::default());
}

#[test]
fn every_name_the_window_publishes_starts_with_the_hosts_prefix() {
    let ctx = crate::test_support::context();
    let mut fake = Fake::default();
    fake.window.open_at(1, PAGES.len());
    let no_prefs_names = |when: &str| {
        assert_eq!(registry::handles("prefs:"), Vec::new(), "{when}: no handle of another window's");
        assert_eq!(registry::rects("prefs:"), Vec::new(), "{when}: no rect of another window's");
    };
    for events in [Vec::new(), Vec::new(), vec![egui::Event::Text("grid".into())], Vec::new(), Vec::new()] {
        frame_as(&ctx, &mut fake, events, "x");
    }
    assert_eq!(fake.window.filter, "grid", "the search shows its clear button and a match");
    for name in ["x:defaults", "x:search", "x:search:clear", "x:search:current", "x:ok", "x:apply", "x:cancel"] {
        assert!(registry::handle_at(name).is_some(), "{name} is published");
    }
    for name in ["x:search:frame", "x:page_body", "x:window", "x:footer", "x:actions"] {
        assert!(registry::rect_of(name).is_some(), "{name} is published");
    }
    no_prefs_names("searching");
    assert!(ctx.memory(|memory| memory.has_focus(search_id(egui::Id::unique(("test_prefs_window", "x"))))), "the field's id is the window's");

    frame_as(&ctx, &mut fake, key(egui::Key::Enter), "x");
    frame_as(&ctx, &mut fake, Vec::new(), "x");
    assert_eq!(fake.window.section, 0, "Enter opened the match");
    assert!(registry::handle_at("x:revealed").is_some(), "the opened row is published under the prefix");
    no_prefs_names("revealing");
}

#[test]
fn invalid_draft_fields_keep_ok_and_apply_editable_until_corrected() {
    for button in [Button::Ok, Button::Apply] {
        let (ctx, mut fake) = opened();
        fake.values.grid = true;
        fake.failing_confirmations = 2;
        let target = if button == Button::Ok { "test:ok" } else { "test:apply" };
        for _ in 0..2 {
            frame(&ctx, &mut fake, Vec::new());
            assert_eq!(click(&ctx, &mut fake, target), Some(button));
            assert!(fake.window.open);
            assert_eq!(fake.window.snapshot(), Some(&Values::default()));
            assert!(fake.window.restore_error().is_none());
        }
        press(&mut fake, PAGES, button).unwrap();
        assert!(fake.values.grid);
        assert_eq!(fake.window.open, button == Button::Apply);
    }
}
