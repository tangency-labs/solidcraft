//! The Navigation page through the Preferences window, with a host that is no app: the page's
//! defaults, its search and Restore Defaults.
use crate::prefs_navigation::*;
use crate::prefs_navigation_page::*;
use crate::prefs_window::{Button, Host, HostPage, Labels, Options, Page, State, press, show};
use crate::test_support::pointer;
use crate::{Icon, settings_search};
use solidcraft_ui_registry as registry;

/// A host with no SolidCraft code: it keeps the settings and the window's state.
#[derive(Default)]
struct NoAppHost {
    navigation: NavigationSettings,
    window: State<NavigationSettings>,
}

impl Host for NoAppHost {
    type Snapshot = NavigationSettings;
    fn window(&mut self) -> &mut State<NavigationSettings> {
        &mut self.window
    }
    fn window_ref(&self) -> &State<NavigationSettings> {
        &self.window
    }
    fn take_snapshot(&self) -> NavigationSettings {
        self.navigation.clone()
    }
    fn matches(&self, snapshot: &NavigationSettings) -> bool {
        *snapshot == self.navigation
    }
    fn restore(&mut self, snapshot: &NavigationSettings) -> Result<(), String> {
        self.navigation = snapshot.clone();
        Ok(())
    }
}

impl PrefsHost for NoAppHost {
    fn navigation(&self) -> NavigationSettings {
        self.navigation.clone()
    }
    fn set_navigation(&mut self, settings: NavigationSettings) {
        self.navigation = settings;
    }
}

const PAGES: &[Page<NoAppHost>] = &[Page::Host(HostPage {
    icon: Icon::Kit("orbit"),
    label: "prefs.navigation",
    group: None,
    handle: "test:page:navigation",
    scroll: true,
    draw: navigation_page::<NoAppHost>,
    defaults: navigation_defaults::<NoAppHost>,
})];

fn options() -> Options<'static, NoAppHost> {
    Options {
        id: egui::Id::unique(("test_navigation_window", "nav")),
        prefix: "test_nav",
        labels: Labels {
            title: "Preferences",
            icon: "settings",
            ok: "OK",
            cancel: "Cancel",
            apply: "Apply",
            restore_defaults_hint: "Reset this page",
        },
        sidebar_footer: None,
        above_page: None,
    }
}

fn frame(ctx: &egui::Context, host: &mut NoAppHost, events: Vec<egui::Event>) -> Option<Button> {
    registry::clear_handles();
    let mut pressed = None;
    crate::test_support::raw_frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        },
        |ui| pressed = show(ui.ctx(), host, PAGES, &options(), |_| {}),
    );
    pressed
}

fn click(ctx: &egui::Context, host: &mut NoAppHost, handle: &str) -> Option<Button> {
    let at = registry::rect_of(handle).map(|rect| rect.center()).or_else(|| registry::handle_at(handle));
    let at = at.unwrap_or_else(|| panic!("{handle} is published"));
    frame(ctx, host, pointer(at, true)).or(frame(ctx, host, pointer(at, false)))
}

fn opened() -> (egui::Context, NoAppHost) {
    let ctx = crate::test_support::context();
    let mut host = NoAppHost::default();
    host.window.open_at(0, PAGES.len());
    for _ in 0..3 {
        frame(&ctx, &mut host, Vec::new());
    }
    (ctx, host)
}

#[test]
fn the_defaults_are_the_settings_the_page_starts_from() {
    let d = NavigationSettings::default();
    assert_eq!(d.nav, "fusion", "the first mouse mapping is the default");
    assert_eq!(d.middle_double_click, "home");
    assert_eq!(d.orbit_around, OrbitAround::ViewCentre);
    assert_eq!(d.navigator_style, NavigatorStyle::Axes);
    assert_eq!(d.trackpad_zoom_modifier, TrackpadZoomModifier::Off);
    assert!(d.zoom_to_cursor && d.right_drag_orbits && d.browser_arrow_keys && d.look_at_anim && d.trackpad_pan);
    assert!(!d.zoom_reverse && !d.free_orbit && !d.radial_context_menu && !d.zoom_stop_at_model);
    assert_eq!((d.wheel_zoom_speed, d.look_at_speed, d.home_fit_size), (1.0, 1.0, 1.0));
    assert_eq!(d.mouse_gestures, MouseGestureSettings::default(), "gestures start off with no lists of their own");
}

#[test]
fn choosing_a_preset_sets_what_the_preset_names_and_leaves_the_rest() {
    let mut s = NavigationSettings::default();
    s.choose_preset(nav_preset("solidworks"));
    assert_eq!((s.nav.as_str(), s.zoom_reverse, s.middle_double_click.as_str(), s.right_drag_orbits), ("solidworks", true, "fit", false));
    s.choose_preset(nav_preset("creo"));
    assert_eq!(s.middle_double_click, "fit", "Creo names no double-click, so it stays as it was");
    assert!(!s.zoom_reverse && !s.right_drag_orbits, "Creo sets the wheel direction and the drag, which it leaves as they were");
}

#[test]
fn the_page_search_finds_its_settings_by_their_labels() {
    let (ctx, mut host) = opened();
    host.window.filter = "zoom".into();
    for _ in 0..3 {
        frame(&ctx, &mut host, Vec::new());
    }
    let (_, _, hits) = settings_search::nav(&ctx);
    let labels: Vec<&str> = hits.iter().map(|hit| hit.label.as_str()).collect();
    assert!(labels.contains(&"Zoom to the cursor"), "search finds the wheel's zoom setting: {labels:?}");
    assert!(labels.contains(&"Reverse zoom direction"), "search finds the zoom direction: {labels:?}");
}

#[test]
fn restore_defaults_resets_the_navigation_page_and_cancel_returns_to_the_snapshot() {
    let (ctx, mut host) = opened();
    host.navigation.zoom_to_cursor = false;
    host.navigation.wheel_zoom_speed = 2.0;
    frame(&ctx, &mut host, Vec::new());
    assert_eq!(click(&ctx, &mut host, "test_nav:defaults"), Some(Button::Defaults));
    assert_eq!(host.navigation, NavigationSettings::default(), "Restore Defaults resets every setting on the page");
    host.navigation.trackpad_pan = false;
    frame(&ctx, &mut host, Vec::new());
    assert_eq!(click(&ctx, &mut host, "test_nav:cancel"), Some(Button::Cancel));
    assert_eq!(host.navigation, NavigationSettings::default(), "Cancel puts the snapshot back");
    assert!(!host.window.open);
    assert!(press(&mut host, PAGES, Button::Ok).is_ok());
}
