//! The application bar from plain data, no app: what each click returns.
use super::*;

const HANDLES: Handles<'static> = Handles {
    home: &["t:home"],
    home_rect: &["t:home"],
    file: &["t:file"],
    file_rect: &["t:file"],
    button: &["t:"],
    separator: "t:separator:",
    search: "t:search",
    report: "t:report",
    pill: "t:",
    settings: "t:settings",
    version: "t:version",
};

fn button(id: &'static str, enabled: bool, wide_only: bool) -> BarButton<'static> {
    BarButton { id, icon: "save", tip: id, enabled, narrow: true, wide_only }
}

/// A bar `width` wide: Save, a disabled Undo (left out of a compact bar), Help, Settings.
struct BarRig {
    ctx: egui::Context,
    width: f32,
    name: &'static str,
    time: std::cell::Cell<f64>,
    out: std::cell::RefCell<Option<BarOut>>,
}

impl BarRig {
    fn new(width: f32) -> BarRig {
        BarRig::named(width, "App")
    }

    /// The bar with the app's name `name`.
    fn named(width: f32, name: &'static str) -> BarRig {
        let rig = BarRig { ctx: crate::test_support::context(), width, name, time: std::cell::Cell::new(0.0), out: Default::default() };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<BarAction> {
        self.frame_output(events).0
    }

    /// A frame: what was chosen, and what was drawn.
    fn frame_output(&self, events: Vec<egui::Event>) -> (Vec<BarAction>, egui::FullOutput) {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 400.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let bar = AppBar {
            items: None,
            quick_ids: &[],
            mark: &|_, _| {},
            name: self.name,
            leading: None,
            home: BarButton { id: "home", icon: "home", tip: "Home", enabled: true, narrow: false, wide_only: false },
            file: "File",
            file_open: false,
            actions: vec![button("save", true, false), button("undo", false, true)],
            quick: 2,
            help: BarButton { id: "help", icon: "help", tip: "Help", enabled: true, narrow: false, wide_only: false },
            settings: BarButton { id: "settings", icon: "settings", tip: "Settings", enabled: true, narrow: false, wide_only: false },
            report: Report { label: "Report", tip: "Report a problem", icon_only: false },
            pills: vec![],
            search: false,
            search_shortcut: Some("S"),
            assistant: true,
            version: Some(Version { text: "v1.0", url: "https://example.invalid/notes" }),
            handles: HANDLES,
        };
        let mut actions = Vec::new();
        let output = crate::test_support::raw_frame(&self.ctx, input, |ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(self.width, HEIGHT));
            let out = show(ui, &bar, rect, rect);
            actions = out.actions.clone();
            actions.extend(overflow_menu(ui.ctx(), &out.overflow_items, out.overflow_button));
            *self.out.borrow_mut() = Some(out);
        });
        (actions, output)
    }

    fn click(&self, name: &str) -> Vec<BarAction> {
        let at = solidcraft_ui_registry::handle_at(name).or_else(|| solidcraft_ui_registry::rect_of(name).map(|r| r.center()));
        let at = at.unwrap_or_else(|| panic!("{name} is drawn"));
        self.time.set(self.time.get() + 1.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(at)]);
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), press(true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), press(false)]));
        actions
    }
}

/// Each control returns its own action, a disabled one nothing; the places left to the app come
/// back in the output.
#[test]
fn the_bar_returns_what_was_clicked() {
    let rig = BarRig::new(1200.0);
    assert_eq!(rig.click("t:home"), vec![BarAction::Home]);
    let file = solidcraft_ui_registry::rect_of("t:file").expect("File is drawn");
    assert_eq!(rig.click("t:file"), vec![BarAction::File { rect: file }]);
    assert_eq!(rig.click("t:save"), vec![BarAction::Button { id: "save".into() }]);
    assert_eq!(rig.click("t:undo"), Vec::new(), "a disabled button runs nothing");
    assert_eq!(rig.click("t:settings"), vec![BarAction::Button { id: "settings".into() }]);
    assert!(matches!(rig.click("t:help").as_slice(), [BarAction::Help { .. }]));
    assert_eq!(rig.click("t:report"), vec![BarAction::Report]);
    assert_eq!(rig.click("t:version"), vec![BarAction::Version]);
    let out = rig.out.borrow();
    let out = out.as_ref().expect("drawn");
    let (first, step) = out.quick.expect("room for the quick-access buttons");
    assert!(first > file.right() && step > 0.0);
    assert!(out.tabs_start > first + step, "the tabs follow the quick-access buttons");
    assert!(out.assistant.is_some_and(|a| a.left() > out.controls_left), "the assistant is in the right cluster");
}

/// At 600 points the name and lower priority End controls yield before Undo and quick access.
#[test]
fn a_compact_bar_keeps_only_what_fits() {
    let rig = BarRig::new(600.0);
    assert!(solidcraft_ui_registry::rect_of("t:save").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:undo").is_some(), "Undo fits after lower priorities give way");
    assert!(rig.out.borrow().as_ref().is_some_and(|out| out.quick.is_some() && out.quick_count == 2));
    assert!(rig.out.borrow().as_ref().is_some_and(|out| out.overflow.contains(&"version".to_string())));
    assert!(solidcraft_ui_registry::rect_of("appbar:overflow").is_some());
    assert!(solidcraft_ui_registry::handle_at("t:version").is_none(), "no version on a narrow bar");
}

/// A long app name ends before Home: Home follows the name as drawn, not a place sized for one
/// name.
#[test]
fn home_follows_a_long_app_name() {
    let name = "A Considerably Longer Application";
    let rig = BarRig::named(1200.0, name);
    let (_, output) = rig.frame_output(Vec::new());
    let text = crate::test_support::text_shapes(&output)
        .find(|shape| shape.galley.text() == name)
        .map(|shape| shape.visual_bounding_rect())
        .expect("the name is drawn");
    let home = solidcraft_ui_registry::rect_of("t:home").expect("Home is drawn");
    assert!(text.right() < home.left(), "the name ends at {}, Home starts at {}", text.right(), home.left());
}

/// Geometry captured from the pre-layout-engine bar, with crowded rows derived from the
/// corrected packing rules below. Bits select search, icon-only
/// report, version, two pills, assistant, five quick buttons, long name and Windows captions.
/// Five widths cross compact/wide and pill-label thresholds; variants include none, each bit,
/// all bits and desktop defaults (28). Pairs cover search with/without pills, report/version,
/// quick/captions and long-name/quick; crowded mixes (188, 254, 255) cover space pressure.
#[test]
fn app_bar_old_show_golden() {
    let mut case = 0;

    for width in [480.0, 640.0, 1000.0, 1280.0, 1920.0] {
        for bits in [0, 1, 2, 4, 8, 16, 32, 64, 128, 255, 28, 3, 5, 6, 9, 17, 24, 33, 65, 96, 129, 160, 188, 254] {
            let ctx = crate::test_support::context();
            let has = |bit| bits & (1 << bit) != 0;
            let bar = AppBar {
                items: None,
                quick_ids: &[],
                mark: &|_, _| {},
                name: if has(6) { "A Considerably Longer Application" } else { "App" },
                leading: None,
                home: button("home", true, false),
                file: "File",
                file_open: false,
                actions: vec![button("save", true, false), button("undo", false, true), button("redo", true, true)],
                quick: if has(5) { 5 } else { 0 },
                help: button("help", true, false),
                settings: button("settings", true, false),
                report: Report { label: "Report a bug", tip: "Report a problem", icon_only: has(1) },
                pills: if has(3) {
                    ["donate", "discord"]
                        .into_iter()
                        .map(|id| Pill {
                            id,
                            icon: "help",
                            label: id,
                            labelled: width >= PILL_LABEL_MIN,
                            tip: id,
                            hide_tip: "Hide",
                            name: id,
                            hide_name: "Hide",
                            fill: egui::Color32::WHITE,
                            ink: egui::Color32::BLACK,
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                search: has(0),
                search_shortcut: Some("Ctrl+K"),
                assistant: has(4),
                version: has(2).then_some(Version { text: "v1.0", url: "https://example.invalid/notes" }),
                handles: HANDLES,
            };
            solidcraft_ui_registry::clear_handles();
            let mut snapshot = Vec::new();
            let number = |v: f32| (v * 1000.0).round() as i32;
            let rect = |r: Option<Rect>| r.map_or_else(Vec::new, |r| vec![number(r.min.x), number(r.min.y), number(r.max.x), number(r.max.y)]);
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 400.0))), ..Default::default() };
            crate::test_support::raw_frame(&ctx, input, |ui| {
                let r = Rect::from_min_size(Pos2::ZERO, vec2(width, HEIGHT));
                let content = Rect::from_min_max(r.min, r.max - vec2(if has(7) { 138.0 } else { 0.0 }, 0.0));
                let out = show(ui, &bar, r, content);
                for name in [
                    "home",
                    "file",
                    "save",
                    "undo",
                    "redo",
                    "help",
                    "separator:brand",
                    "separator:quick_access",
                    "separator:documents",
                    "search",
                    "report",
                    "donate",
                    "donate:hide",
                    "discord",
                    "discord:hide",
                    "settings",
                    "version",
                ] {
                    let name = format!("t:{name}");
                    snapshot.push(rect(solidcraft_ui_registry::rect_of(&name)));
                    snapshot.push(solidcraft_ui_registry::handle_at(&name).map_or_else(Vec::new, |p| vec![number(p.x), number(p.y)]));
                }
                snapshot.push(out.quick.map_or_else(Vec::new, |(x, step)| vec![number(x), number(step)]));
                snapshot.push(rect(Some(out.offer)));
                snapshot.push(vec![i32::from(out.offer_labeled)]);
                snapshot.push(vec![number(out.tabs_start), number(out.controls_left)]);
                snapshot.push(rect(out.search));
                snapshot.push(rect(out.assistant));
                snapshot.push(rect(out.leading));
                snapshot.push(rect(Some(out.file)));
                assert!(out.actions.is_empty(), "idle bar returned actions at {width}, variant {bits}: {:?}", out.actions);
            });
            let mut expected = OLD_BAR_ROWS[case];
            for (field, actual) in snapshot.iter().enumerate() {
                let (&len, rest) = expected.split_first().expect("recorded field length");
                assert_eq!(actual.len(), len as usize, "width {width}, variant {bits}, field {field}: presence");
                let (values, rest) = rest.split_at(actual.len());
                for (&a, &e) in actual.iter().zip(values) {
                    assert!((a - e).abs() <= 1, "width {width}, variant {bits}, field {field}: {actual:?} != {values:?}");
                }
                expected = rest;
            }
            assert!(expected.is_empty(), "width {width}, variant {bits}: unchecked fields");
            case += 1;
        }
    }
}
const TAB_HANDLES: TabHandles<'static> = TabHandles {
    tab: "t:tab:",
    close: "t:close:",
    more: "t:more",
    new: "t:new",
    rename: "t:rename",
    menu_row: "t:menu:",
    more_row: "t:list:",
    view: "t:view:",
    view_close: "t:vclose:",
    view_new: "t:vnew",
    view_more: "t:vmore",
    switcher: "t:switcher",
};

const VIEW_MENU: [TabMenuRow<'static>; 4] = [
    TabMenuRow::new("move_to_window", "Move to window", "").for_local_view().with_children(&[TabMenuRow::new("move_new", "New window", "")]),
    TabMenuRow::new("vclose", "Close view", "Ctrl+W"),
    TabMenuRow::new("vothers", "Close other views", "").needing_others().separated(),
    TabMenuRow::new("vright", "Close views to the right", "").needing_others(),
];
const PLUS_MENU: [TabMenuRow<'static>; 4] = [
    TabMenuRow::heading("Open as a view"),
    TabMenuRow::new("plus_sketch", "Sketch", "").with_icon("sketch", Some(egui::Color32::GREEN)),
    TabMenuRow::new("plus_dup", "Duplicate this view", "").with_icon("duplicate", None).separated(),
    TabMenuRow::new("plus_reopen", "Reopen closed view", "Ctrl+Shift+T").with_icon("reopen", None),
];
const SWITCHER_MENU: [TabMenuRow<'static>; 1] = [TabMenuRow::new("as_tabs", "Show views as tabs", "")];

const VIEWS: [ViewTab<'static>; 3] = [
    ViewTab {
        name: "Model",
        icon: "model",
        accent: egui::Color32::BLUE,
        underline: egui::Color32::BLUE,
        state: ViewState::Normal,
        active: true,
        to_window: None,
        tip: "Model",
    },
    ViewTab {
        name: "Sketch 2",
        icon: "sketch",
        accent: egui::Color32::GREEN,
        underline: egui::Color32::GREEN,
        state: ViewState::SourceChanged,
        active: false,
        to_window: None,
        tip: "Sketch 2",
    },
    ViewTab {
        name: "Sheet 1",
        icon: "sheet",
        accent: egui::Color32::RED,
        underline: egui::Color32::RED,
        state: ViewState::Deleted,
        active: false,
        to_window: None,
        tip: "Sheet 1",
    },
];

const TAB_MENU: [TabMenuRow<'static>; 2] = [TabMenuRow::new("close", "Close", ""), TabMenuRow::new("others", "Close Others", "").needing_others()];

/// Three document tabs, the first selected, in a strip 600 wide.
struct TabsRig {
    ctx: egui::Context,
    time: std::cell::Cell<f64>,
    /// The first design has the three [`VIEWS`], shown as sub-tabs (`Some(true)`) or behind a
    /// switcher (`Some(false)`); the strip then has more room.
    views: Option<bool>,
    room: Option<f32>,
    single_view: bool,
    name: &'static str,
    detached: bool,
    pop_out: bool,
    /// The + has a menu.
    plus: bool,
    /// The name of this view is being typed: its place, and the text and focus the strip edits.
    rename: Option<usize>,
    text: std::cell::RefCell<String>,
    focused: std::cell::Cell<bool>,
}

impl TabsRig {
    fn new() -> TabsRig {
        TabsRig::with(None)
    }

    fn with(views: Option<bool>) -> TabsRig {
        TabsRig::build(views, false, None)
    }

    /// A rig with `views` whose + opens a menu when `plus`, and view `rename` (with `text`) being renamed.
    fn build(views: Option<bool>, plus: bool, rename: Option<(usize, &str)>) -> TabsRig {
        let text = rename.map_or(String::new(), |(_, text)| text.to_string());
        let rig = TabsRig {
            ctx: crate::test_support::context(),
            time: std::cell::Cell::new(0.0),
            views,
            room: None,
            single_view: false,
            name: "One",
            detached: false,
            pop_out: false,
            plus,
            rename: rename.map(|(k, _)| k),
            text: std::cell::RefCell::new(text),
            focused: std::cell::Cell::new(false),
        };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<TabAction> {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let tab = |name: &'static str| DocTab { name, list_name: name, dirty: false, place: "Not saved yet", security: None, views: &[] };
        let mut views = VIEWS;
        if self.detached {
            views[1].to_window = Some(1);
        }
        let views = if self.single_view || self.pop_out {
            &views[..1]
        } else if self.views.is_some() {
            &views[..]
        } else {
            &[]
        };
        let first = DocTab { views, ..tab(self.name) };
        let (mut text, mut focused) = (self.text.borrow().clone(), self.focused.get());
        let mut strip = TabStrip {
            tabs: if self.pop_out { vec![first] } else { vec![first, tab("Two"), tab("Three")] },
            selected: 0,
            shown_selected: Some(0),
            pop_out: self.pop_out,
            title_bar: false,
            double_click_renames: true,
            rename: None,
            menu: &TAB_MENU,
            close_name: "Close tab",
            close_tip: "Close",
            more_tip: "All",
            new_tip: "New",
            id: egui::Id::unique("test_tabs"),
            handles: TAB_HANDLES,
            views_as_tabs: self.views == Some(true),
            switcher_always: false,
            view_text: ViewText {
                close_view: "Close view",
                close_design: "Close design",
                new_view: "Open another view",
                more: "{n} more",
                switch_view: "Switch view",
            },
            view_menu: &VIEW_MENU,
            view_right_row: "vright",
            view_rename: self.rename.map(|k| (k, TabRename { text: &mut text, focused: &mut focused, max: 8 })),
            plus_menu: if self.plus { &PLUS_MENU } else { &[] },
            layout_menu: &[],
            layout_current: "",
            layout_tip: "Layout",
            layout_fill: "",
            layout_fill_heading: "",
            switcher_menu: &SWITCHER_MENU,
            tear_off_below: 40.0,
        };
        let (x0, x1) = if self.views.is_some() { (20.0, 780.0) } else { (100.0, 700.0) };
        let x1 = self.room.map_or(x1, |room| x0 + room);
        let mut actions = Vec::new();
        crate::test_support::raw_frame(&self.ctx, input, |ui| {
            actions = tabs(ui, &mut strip, Rect::from_min_size(Pos2::ZERO, vec2(800.0, HEIGHT)), x0, x1);
        });
        *self.text.borrow_mut() = text;
        self.focused.set(focused);
        actions
    }

    fn press(&self, at: Pos2, button: egui::PointerButton) -> Vec<TabAction> {
        self.time.set(self.time.get() + 1.0);
        let event = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(at)]);
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), event(true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), event(false)]));
        actions
    }

    fn at(&self, name: &str) -> Pos2 {
        solidcraft_ui_registry::rect_of(name)
            .map(|r| r.center())
            .or_else(|| solidcraft_ui_registry::handle_at(name))
            .unwrap_or_else(|| panic!("{name} is drawn"))
    }
}

/// A click selects a tab, its × closes it, + asks for a new one, a row of a tab's menu returns its
/// id, and a tab dragged past another is moved there.
#[test]
fn the_tab_strip_returns_what_was_clicked_and_dragged() {
    let rig = TabsRig::new();
    assert_eq!(rig.press(rig.at("t:tab:1"), egui::PointerButton::Primary), vec![TabAction::Select(1)]);
    assert_eq!(rig.press(rig.at("t:close:0"), egui::PointerButton::Primary), vec![TabAction::Close(0)]);
    assert_eq!(rig.press(rig.at("t:new"), egui::PointerButton::Primary), vec![TabAction::New]);
    rig.press(rig.at("t:tab:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:others"), egui::PointerButton::Primary), vec![TabAction::Menu { tab: 2, id: "others".into() }]);
    // Drag the first tab past the last.
    let from = rig.at("t:tab:0");
    let to = solidcraft_ui_registry::rect_of("t:tab:2").map(|r| r.right_center() - vec2(4.0, 0.0)).expect("tab 2 is drawn");
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    actions.extend(rig.frame(vec![button(from, true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![button(to, false)]));
    assert_eq!(actions, vec![TabAction::Reorder { from: 0, to: 2 }]);
}

fn file_item(id: &str, enabled: bool) -> FileItem {
    FileItem { id: id.to_string(), label: format!("Item {id}"), key: String::new(), icon: "open".to_string(), enabled }
}

/// The File menu drawn for a frame with `events`: what it returned, and where each text was.
fn file_menu_frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> (FileMenuOut, Vec<(String, Pos2)>) {
    let rows = vec![FileRow::Item(file_item("new", true)), FileRow::Item(file_item("save", false)), FileRow::Separator];
    file_menu_frame_of(ctx, &rows, 600.0, time, events)
}

/// The File menu with `rows` in an 800 by `height` window, drawn for a frame with `events`.
fn file_menu_frame_of(ctx: &egui::Context, rows: &[FileRow], height: f32, time: f64, events: Vec<egui::Event>) -> (FileMenuOut, Vec<(String, Pos2)>) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, height))),
        time: Some(time),
        events,
        focused: true,
        ..Default::default()
    };
    let mut out = FileMenuOut::default();
    let output = crate::test_support::raw_frame(ctx, input, |ui| {
        out = file_menu(ui.ctx(), rows, pos2(100.0, HEIGHT), egui::Id::unique("test_file_menu"), false);
    });
    let texts = crate::test_support::text_shapes(&output).map(|s| (s.galley.text().to_string(), s.visual_bounding_rect().center())).collect();
    (out, texts)
}

/// A File menu taller than the window stays inside it, and its last row scrolls into view.
#[test]
fn a_file_menu_taller_than_the_window_scrolls_its_rows() {
    let ctx = crate::test_support::context();
    let rows: Vec<FileRow> = (0..40).map(|i| FileRow::Item(file_item(&format!("r{i}"), true))).collect();
    let height = 360.0;
    file_menu_frame_of(&ctx, &rows, height, 0.0, Vec::new());
    file_menu_frame_of(&ctx, &rows, height, 0.1, Vec::new());
    let menu = ctx.memory(|m| m.area_rect(egui::Id::unique("test_file_menu"))).expect("the menu is laid out");
    assert!(menu.bottom() <= height, "the menu ends at {} inside a {height} high window", menu.bottom());
    let shown = |texts: &[(String, Pos2)], text: &str| texts.iter().any(|(t, p)| t == text && menu.contains(*p));
    let over = menu.center();
    let mut texts = Vec::new();
    for i in 0..20 {
        let wheel = egui::Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        (_, texts) = file_menu_frame_of(&ctx, &rows, height, 0.2 + f64::from(i) * 0.1, vec![egui::Event::PointerMoved(over), wheel]);
    }
    assert!(shown(&texts, "Item r39"), "scrolling shows the last row");
    assert!(!shown(&texts, "Item r0"), "and the first has scrolled out");
}

/// A row returns its id and closes the menu; a disabled one returns nothing and keeps it open;
/// a click elsewhere closes it.
#[test]
fn the_file_menu_returns_the_row_chosen() {
    let ctx = crate::test_support::context();
    let mut time = 0.0;
    let mut click = |at: Pos2| {
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut last = FileMenuOut::default();
        for events in [vec![egui::Event::PointerMoved(at)], vec![egui::Event::PointerMoved(at), press(true)], vec![press(false)]] {
            time += 0.5;
            let (out, _) = file_menu_frame(&ctx, time, events);
            if out.chosen.is_some() || out.close {
                last = out;
            }
        }
        last
    };
    // An area is laid out on its first frame and painted from the next.
    file_menu_frame(&ctx, 0.0, Vec::new());
    let (_, texts) = file_menu_frame(&ctx, 0.1, Vec::new());
    let at = |text: &str| texts.iter().find(|(t, _)| t == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("{text} is painted"));
    let (new, save) = (at("Item new"), at("Item save"));
    assert_eq!(click(save), FileMenuOut::default(), "a disabled row chooses nothing and the menu stays");
    assert_eq!(click(new), FileMenuOut { chosen: Some("new".into()), close: true });
    assert_eq!(click(pos2(700.0, 500.0)), FileMenuOut { chosen: None, close: true }, "a click elsewhere closes it");
}

/// The switcher returns its click and publishes where it is.
#[test]
fn the_workspace_switcher_returns_its_click() {
    let ctx = crate::test_support::context();
    let ws = Rect::from_min_size(Pos2::ZERO, vec2(88.0, 117.0));
    let data = Switcher {
        name: "Design",
        caption: "Workspace",
        icon: "ws_design",
        tip: "Change",
        open: false,
        floating: false,
        labels: true,
        folded: false,
        stage: false,
        accent: Tokens::get().accent,
        handle_rect: "t:ws",
        handle_target: "t:ws_target",
    };
    let mut clicked = Vec::new();
    for (i, events) in
        [vec![egui::Event::PointerMoved(ws.center())], vec![press(ws.center(), true)], vec![press(ws.center(), false)]].into_iter().enumerate()
    {
        let input = egui::RawInput { time: Some(i as f64 * 0.1), events, focused: true, ..Default::default() };
        crate::test_support::raw_frame(&ctx, input, |ui| clicked.push(switcher(ui, &data, ws)));
    }
    assert_eq!(clicked, [false, false, true]);
    assert_eq!(solidcraft_ui_registry::rect_of("t:ws"), Some(ws));
}

fn press(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

/// A design with several views is a pill where the app asks for sub-tabs and the bar has room for
/// the active view and the "N more" button; otherwise a switcher. One view or none is a plain tab,
/// unless the app asks for the switcher always: then one view has it too, but never with sub-tabs.
#[test]
fn the_selected_tab_falls_back_to_a_switcher_where_the_pill_has_no_room() {
    assert_eq!(selected_layout(0, true, false, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(1, true, false, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(1, false, false, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(3, true, false, 300.0, 300.0), SelectedLayout::Pill);
    assert_eq!(selected_layout(3, true, false, 300.0, 299.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(3, false, false, 300.0, 900.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(1, false, true, 300.0, 900.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(1, true, true, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(0, false, true, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(3, true, true, 300.0, 300.0), SelectedLayout::Pill);
}

/// The sub-tabs all show when they fit; otherwise a run of neighbours that holds the shown view
/// stays and the "N more" button takes its room.
#[test]
fn a_pill_that_is_too_narrow_keeps_the_active_view_in_sight() {
    let widths = [60.0; 6];
    let fits = 6.0 * (60.0 + VIEW_GAP);
    assert_eq!(view_window(&widths, 3, fits, 50.0), (0, 6));
    // Room for three sub-tabs and the button: the run starts at the first that lets the active one in.
    let room = 3.0 * (60.0 + VIEW_GAP) + 50.0;
    assert_eq!(view_window(&widths, 1, room, 50.0), (0, 3));
    assert_eq!(view_window(&widths, 4, room, 50.0), (2, 3));
    assert_eq!(view_window(&widths, 5, room, 50.0), (3, 3));
    // Less room than one sub-tab: the active view still shows.
    assert_eq!(view_window(&widths, 4, 10.0, 50.0), (4, 1));
    assert_eq!(view_window(&[], 0, -5.0, 50.0), (0, 0));
}

/// A design tab is torn off only when released well below the bar, and never where the app turns it off.
#[test]
fn a_tab_is_torn_off_only_well_below_the_bar() {
    assert!(!is_tear_off(40.0, 34.0, 40.0));
    assert!(!is_tear_off(74.0, 34.0, 40.0));
    assert!(is_tear_off(75.0, 34.0, 40.0));
    assert!(!is_tear_off(500.0, 34.0, 0.0));
}

/// A sub-tab dropped between others lands at the slot nearest its left edge, and the marker sits
/// where the others leave room for it.
#[test]
fn a_view_dragged_along_its_design_lands_in_the_nearest_slot() {
    let widths = [50.0, 70.0, 60.0];
    // Dragging the first sub-tab: the others are 70 and 60 wide.
    assert_eq!(drop_slot(0.0, 10.0, &widths, 0, 0), 0);
    assert_eq!(drop_slot(85.0, 10.0, &widths, 0, 0), 1);
    assert_eq!(drop_slot(400.0, 10.0, &widths, 0, 0), 2);
    assert_eq!(slot_left(10.0, &widths, 0, 0, 0), 10.0);
    assert_eq!(slot_left(10.0, &widths, 0, 0, 1), 80.0);
    assert_eq!(slot_left(10.0, &widths, 0, 0, 2), 140.0);
    // A window that starts at the second sub-tab counts places from there.
    assert_eq!(slot_left(10.0, &widths[1..], 1, 2, 1), 10.0);
    assert_eq!(slot_left(10.0, &widths[1..], 1, 2, 2), 80.0);
}

/// The sub-tabs of the selected design report a click, the close button, the + (no menu: a new
/// view), a right-click menu row and a drag to a new place.
#[test]
fn the_sub_tabs_return_what_was_clicked_and_dragged() {
    let rig = TabsRig::with(Some(true));
    assert_eq!(rig.press(rig.at("t:view:1"), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
    assert_eq!(rig.press(rig.at("t:vclose:0"), egui::PointerButton::Primary), vec![TabAction::CloseView(0)]);
    assert_eq!(rig.press(rig.at("t:vnew"), egui::PointerButton::Primary), vec![TabAction::NewView]);
    assert_eq!(rig.press(rig.at("t:close:0"), egui::PointerButton::Primary), vec![TabAction::Close(0)]);
    rig.press(rig.at("t:view:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:vclose"), egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 2, id: "vclose".into() }]);
    // Drag the first sub-tab past the last.
    let origin = solidcraft_ui_registry::rect_of("t:view:0").expect("view 0 is drawn");
    let from = origin.center();
    let to = solidcraft_ui_registry::rect_of("t:view:2").map(|r| r.right_center() - vec2(4.0, 0.0)).expect("view 2 is drawn");
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    actions.extend(rig.frame(vec![press(from, true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(3.0, 0.0))]));
    assert!(actions.is_empty(), "three pixels stay a click");
    let pointer = from + vec2(4.0, 0.0);
    assert_eq!(rig.frame(vec![egui::Event::PointerMoved(pointer)]), vec![TabAction::DockView { view: 0, origin, pointer, released: false }]);
    assert_eq!(rig.frame(vec![egui::Event::PointerMoved(to)]), vec![TabAction::DockView { view: 0, origin, pointer: to, released: false }]);
    assert_eq!(
        rig.frame(vec![press(to, false)]),
        vec![TabAction::DockView { view: 0, origin, pointer: to, released: true }, TabAction::ReorderView { from: 0, to: 2 }]
    );
}

#[test]
fn a_view_drag_reports_release_outside_the_client_and_escape_cancels_it() {
    let rig = TabsRig::with(Some(true));
    let origin = solidcraft_ui_registry::rect_of("t:view:0").expect("view 0 is drawn");
    let from = origin.center();
    let outside = pos2(850.0, from.y);
    let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    for cancel in [false, true] {
        rig.frame(vec![egui::Event::PointerMoved(from)]);
        rig.frame(vec![press(from, true)]);
        rig.frame(vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))]);
        assert!(rig.frame(vec![egui::Event::PointerMoved(outside)]).contains(&TabAction::DockView {
            view: 0,
            origin,
            pointer: outside,
            released: false
        }));
        if cancel {
            assert!(rig.frame(vec![escape.clone()]).is_empty());
            assert!(rig.frame(vec![egui::Event::PointerMoved(outside + vec2(4.0, 0.0))]).is_empty(), "a cancelled press stays cancelled");
        }
        let actions = rig.frame(vec![press(outside, false)]);
        assert_eq!(actions.contains(&TabAction::DockView { view: 0, origin, pointer: outside, released: true }), !cancel);
    }
}

#[test]
fn touch_lifts_a_view_tab_only_after_the_hold_threshold() {
    let rig = TabsRig::with(Some(true));
    let origin = solidcraft_ui_registry::rect_of("t:view:0").expect("view 0 is drawn");
    let pointer = origin.center();
    let touch = |phase| egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(1), phase, pos: pointer, force: None };
    rig.frame(vec![egui::Event::PointerMoved(pointer)]);
    assert!(rig.frame(vec![touch(egui::TouchPhase::Start), press(pointer, true)]).is_empty());
    rig.time.set(rig.time.get() + 0.200);
    assert!(rig.frame(Vec::new()).is_empty());
    rig.time.set(rig.time.get() + 0.025);
    assert_eq!(rig.frame(Vec::new()), vec![TabAction::DockView { view: 0, origin, pointer, released: false }]);
    assert_eq!(
        rig.frame(vec![touch(egui::TouchPhase::End), press(pointer, false)]),
        vec![TabAction::DockView { view: 0, origin, pointer, released: true }]
    );
}

#[test]
fn space_lifts_a_focused_local_view_tab_without_selecting_it() {
    let rig = TabsRig::with(Some(true));
    let origin = solidcraft_ui_registry::rect_of("t:view:1").expect("view 1 is drawn");
    assert_eq!(rig.press(origin.center(), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
    let space = egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    assert_eq!(rig.frame(vec![space]), vec![TabAction::LiftView { view: 1, origin }]);
}

#[test]
fn a_view_drop_slot_shifts_only_neighbours_at_or_after_the_local_index() {
    let rig = TabsRig::with(Some(true));
    rig.ctx.global_style_mut(|style| style.animation_time = 0.0);
    let strip = egui::Id::unique("test_tabs");
    let first = rig.at("t:view:0");
    let second = rig.at("t:view:1");
    let second_rect = solidcraft_ui_registry::rect_of("t:view:1").unwrap();
    let plus = rig.at("t:vnew");
    set_view_drag_slot(&rig.ctx, strip, Some(1), 38.0);
    rig.frame(Vec::new());
    assert_eq!(rig.at("t:view:0"), first);
    assert_eq!(rig.at("t:view:1"), second + vec2(38.0 + VIEW_GAP, 0.0));
    let gap = view_drag_gap(&rig.ctx, strip, 1).unwrap();
    assert_eq!(gap.left_top(), second_rect.left_top());
    assert_eq!(gap.size(), vec2(38.0, VIEW_HEIGHT));
    assert_eq!(rig.at("t:vnew"), plus + vec2(38.0 + VIEW_GAP, 0.0));
    set_view_drag_slot(&rig.ctx, strip, None, 0.0);
    rig.frame(Vec::new());
    assert_eq!(rig.at("t:view:1"), second);
    assert_eq!(rig.at("t:vnew"), plus);
    assert!(view_drag_gap(&rig.ctx, strip, 1).is_none());
}

/// With a view switcher the views list in a menu: a row picks a view, the footer's rows are the app's.
#[test]
fn the_view_switcher_lists_every_view() {
    let rig = TabsRig::with(Some(false));
    assert!(solidcraft_ui_registry::rect_of("t:view:0").is_none(), "no sub-tabs behind a switcher");
    rig.press(rig.at("t:switcher"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:switcher:row:2"), egui::PointerButton::Primary), vec![TabAction::SelectView(2)]);
    rig.press(rig.at("t:switcher"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:as_tabs"), egui::PointerButton::Primary), vec![TabAction::SwitcherMenu { id: "as_tabs".into() }]);
}

/// A click on the + opens its menu and leaves it open: two frames later its rows are drawn under the
/// +'s left edge, and a row returns its id. A press in the frame that opened the menu is not a press
/// outside it.
#[test]
fn the_plus_menu_stays_open_after_its_click() {
    let rig = TabsRig::build(Some(true), true, None);
    let plus = solidcraft_ui_registry::rect_of("t:vnew").expect("the + is drawn");
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let row = solidcraft_ui_registry::handle_at("t:menu:plus_sketch").expect("the menu's row is drawn");
    assert!(row.y > HEIGHT && row.x >= plus.left(), "the menu opens under the + at {row:?}");
    assert_eq!(rig.press(row, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_sketch".into() }]);
    // A press and release in one frame (a click driven by a script) leaves the menu open too.
    let at = rig.at("t:vnew");
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    rig.frame(vec![button(true), button(false)]);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:plus_sketch").is_some());
}

/// The + menu's header is drawn but never chosen, every row (with a glyph or without) returns its
/// id, and each row is published at its place under the header.
#[test]
fn the_plus_menu_has_a_header_icon_rows_and_a_separator() {
    let rig = TabsRig::build(Some(true), true, None);
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:Open as a view").is_none(), "the header is not a row");
    let (sketch, dup) = (rig.at("t:menu:plus_sketch"), rig.at("t:menu:plus_dup"));
    assert!(dup.y - sketch.y > 32.0, "a separator sits above Duplicate: {sketch:?} {dup:?}");
    assert_eq!(rig.press(dup, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_dup".into() }]);
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:plus_reopen"), egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_reopen".into() }]);
}

/// The row builders set only what they say, and the glyphs the menus use exist.
#[test]
fn menu_row_builders_and_glyphs() {
    let plain = TabMenuRow::new("a", "A", "Ctrl+A");
    assert!(!plain.heading && !plain.separator_before && !plain.needs_others && plain.icon.is_empty() && plain.accent.is_none());
    let head = TabMenuRow::heading("H");
    assert!(head.heading && head.id == "H" && head.label == "H");
    let row = plain.separated().needing_others().with_icon("close", None);
    assert!(row.separator_before && row.needs_others && row.icon == "close");
    for glyph in ["model", "sketch", "sheet", "render", "animation", "rename", "window", "close", "duplicate", "tiles", "reopen"] {
        assert!(!glyph_lines(glyph).is_empty(), "{glyph} has strokes");
    }
}

/// A view's menu opens under that view's left edge with its key hints, on any view; the rows that
/// need other views are off for a design's only view, and "to the right" is off for its last view.
#[test]
fn a_view_menu_opens_under_its_view_and_turns_off_rows_that_do_not_apply() {
    let rig = TabsRig::with(Some(true));
    let view = solidcraft_ui_registry::rect_of("t:view:1").expect("view 1 is drawn");
    rig.press(rig.at("t:view:1"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let close = solidcraft_ui_registry::handle_at("t:menu:vclose").expect("the menu is drawn");
    assert!(close.y > HEIGHT && close.x >= view.left(), "the menu opens under view 1 at {close:?}");
    assert_eq!(rig.press(rig.at("t:menu:vright"), egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 1, id: "vright".into() }]);
    // The last view has no views to its right: that row does nothing.
    rig.press(rig.at("t:view:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(rig.press(rig.at("t:menu:vright"), egui::PointerButton::Primary).is_empty());
    rig.press(rig.at("t:view:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:vothers"), egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 2, id: "vothers".into() }]);
}

#[test]
fn detached_view_has_a_window_mark_and_retains_its_own_click_targets() {
    let mut rig = TabsRig::with(Some(true));
    let normal = solidcraft_ui_registry::rect_of("t:view:1").expect("view is drawn");
    rig.detached = true;
    rig.frame(Vec::new());
    let detached = solidcraft_ui_registry::rect_of("t:view:1").expect("detached view stays in the strip");
    let mark = solidcraft_ui_registry::rect_of("t:view:1:window").expect("detached view has a window mark");
    let close = solidcraft_ui_registry::rect_of("t:vclose:1").expect("detached view can still be closed");
    assert!(detached.width() > normal.width());
    assert!(detached.contains_rect(mark) && detached.contains_rect(close));
    assert!(mark.right() < close.left(), "the window mark does not cover the close button");
    assert_eq!(rig.press(mark.center(), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
    assert_eq!(rig.press(close.center(), egui::PointerButton::Primary), vec![TabAction::CloseView(1)]);
    rig.press(mark.center(), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:vclose").is_some());
    assert!(solidcraft_ui_registry::handle_at("t:menu:move_to_window").is_none());
}

#[test]
fn pop_out_pill_keeps_one_visible_view_with_no_design_close() {
    let mut rig = TabsRig::with(Some(false));
    rig.pop_out = true;
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:view:0").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:vclose:0").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:vnew").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:close:0").is_none());
    assert!(solidcraft_ui_registry::handle_at("t:new").is_none());
}

#[test]
fn view_window_submenu_returns_the_child_action() {
    let rig = TabsRig::with(Some(true));
    rig.press(rig.at("t:view:1"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let row = rig.at("t:menu:move_to_window");
    assert!(rig.press(row, egui::PointerButton::Primary).is_empty());
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let child = rig.at("t:menu:move_new");
    assert!(child.x > row.x, "the submenu opens beside the parent row");
    assert_eq!(rig.press(child, egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 1, id: "move_new".into() }]);
}

/// A view being renamed shows its name field in its sub-tab: Enter keeps the name (cut to the
/// longest taken), Escape gives it up.
#[test]
fn a_view_is_renamed_in_its_sub_tab() {
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let rig = TabsRig::build(Some(true), false, Some((1, "A much longer name")));
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:rename").is_some(), "the field is drawn in the sub-tab");
    assert_eq!(rig.text.borrow().chars().count(), 8, "the name is cut to the longest taken");
    assert_eq!(rig.frame(vec![key(egui::Key::Enter)]), vec![TabAction::ViewRenameDone { view: 1 }]);
    let rig = TabsRig::build(Some(true), false, Some((2, "Sheet 1")));
    rig.frame(Vec::new());
    assert_eq!(rig.frame(vec![key(egui::Key::Escape)]), vec![TabAction::ViewRenameCancelled]);
}

#[test]
fn a_narrow_switcher_keeps_its_targets_inside_the_tab() {
    let mut rig = TabsRig::with(Some(false));
    rig.room = Some(304.0);
    rig.name = "A design name much wider than the space left for its tab";
    rig.frame(Vec::new());
    let switcher = solidcraft_ui_registry::rect_of("t:switcher").expect("switcher remains available");
    let close = solidcraft_ui_registry::rect_of("t:close:0").expect("close remains available");
    assert!(switcher.is_positive() && switcher.left() >= 20.0 && switcher.right() <= 108.0);
    assert!(close.right() <= 108.0 && !close.intersects(switcher));
    assert!(rig.press(switcher.center(), egui::PointerButton::Primary).is_empty());
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:switcher:row:1"), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
}

#[test]
fn renaming_a_view_remains_editable_in_the_switcher_fallback() {
    let mut rig = TabsRig::build(Some(true), false, Some((1, "Sketch")));
    rig.room = Some(304.0);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:switcher").is_some(), "the narrow strip falls back to its switcher");
    let field = solidcraft_ui_registry::rect_of("t:rename").expect("the requested editor remains visible");
    let switcher = solidcraft_ui_registry::rect_of("t:switcher").unwrap();
    assert!(switcher.contains_rect(field));
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    assert_eq!(rig.frame(vec![key(egui::Key::Enter)]), vec![TabAction::ViewRenameDone { view: 1 }]);
    let mut single = TabsRig::build(Some(false), false, Some((0, "Model")));
    single.single_view = true;
    single.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:rename").is_some(), "the single-view tab exposes its requested rename field");
    assert_eq!(single.frame(vec![key(egui::Key::Enter)]), vec![TabAction::ViewRenameDone { view: 0 }]);
    let rig = TabsRig::build(Some(false), false, Some((2, "Sheet")));
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:rename").is_some(), "switcher mode can rename too");
    assert_eq!(rig.frame(vec![key(egui::Key::Escape)]), vec![TabAction::ViewRenameCancelled]);
}

#[test]
fn changing_the_design_discards_its_open_view_popup() {
    let rig = TabsRig::with(Some(true));
    rig.press(rig.at("t:view:1"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:vclose").is_some());
    clear_view_popup(&rig.ctx, egui::Id::unique("test_tabs"));
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:vclose").is_none());
}

// The layout engine: pure arithmetic over a spec, no egui.
use crate::app_bar_layout::{Item, MAX_ITEMS, Placed, Spec, Zone, default_bar, layout};

const EPS: f32 = 0.05;

/// The default bar `width` wide with Windows' caption buttons on the right.
fn default_spec(width: f32) -> Spec {
    default_bar::spec(width, 0.0, 138.0)
}

fn index_of(spec: &Spec, id: &str) -> usize {
    spec.items.iter().position(|item| item.id == id).expect("the item is in the spec")
}

/// Every placement lies in the bar's room, none has a negative or non-finite width, and no two
/// overlap (the overflow button and the centre item included).
fn assert_sound(spec: &Spec, placed: &Placed) {
    let clamp = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, crate::app_bar_layout::MAX_WIDTH) };
    let (left, right) = (clamp(spec.captions_left), clamp(spec.width) - clamp(spec.captions_right));
    assert_eq!(placed.placements.len(), spec.items.len());
    let mut spans: Vec<(f32, f32, String)> = Vec::new();
    for (p, item) in placed.placements.iter().zip(&spec.items) {
        if let Some(p) = p {
            assert!(p.width.is_finite() && p.width >= 0.0 && p.x.is_finite(), "{}: {p:?}", item.id);
            spans.push((p.x, p.right(), item.id.clone()));
        }
    }
    if let Some((x, w)) = placed.overflow_button {
        assert!(w >= 0.0 && x.is_finite());
        spans.push((x, x + w, "overflow".into()));
    }
    // An item narrower than the tolerance takes no room (its width is lost in the float rounding
    // of a wide neighbour), so it may sit anywhere inside the others' span.
    spans.retain(|(x, r, _)| r - x > EPS);
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (x, r, id) in &spans {
        assert!(*x >= left - EPS && *r <= right.max(left) + EPS, "{id} spans {x}..{r} outside {left}..{right}");
    }
    for pair in spans.windows(2) {
        assert!(pair[0].1 <= pair[1].0 + EPS, "{} ({}) overlaps {} ({})", pair[0].2, pair[0].1, pair[1].2, pair[1].0);
    }
}

/// The default bar never overlaps and stays inside its room at any width, with the caption
/// buttons on the right (Windows) or the traffic lights on the left (macOS).
#[test]
fn the_default_bar_never_overlaps_at_any_width() {
    for width in 200..=3840 {
        let spec = default_spec(width as f32);
        assert_sound(&spec, &layout(&spec));
        let mac = default_bar::spec(width as f32, 78.0, 0.0);
        assert_sound(&mac, &layout(&mac));
    }
}

/// On a wide bar every item has its fullest step, Start items run left to right in today's order
/// and End items likewise, with the tabs between them.
#[test]
fn a_wide_bar_shows_everything_in_todays_order() {
    let spec = default_spec(3840.0);
    let placed = layout(&spec);
    assert!(placed.overflow.is_empty() && placed.overflow_button.is_none());
    let mut last = f32::MIN;
    for zone in [Zone::Start, Zone::Centre, Zone::End] {
        for item in spec.items.iter().filter(|i| i.zone == zone) {
            let p = placed.placements[index_of(&spec, &item.id)].expect("placed on a wide bar");
            assert_eq!(p.step, 0, "{} is at its fullest", item.id);
            if zone != Zone::Centre {
                assert!((p.width - item.ladder[0]).abs() < EPS, "{} keeps its fullest width", item.id);
            }
            assert!(p.x >= last - EPS, "{} is out of order ({} < {last})", item.id, p.x);
            last = p.right();
        }
    }
    let (x, w) = placed.centre.expect("the tabs have a place");
    assert!(w > 1000.0, "the tabs take the spare room, got {w}");
    assert!((x - (108.0 + 32.0 + 24.0 + 62.0 + 3.0 * 34.0)).abs() < EPS, "the tabs start after the file actions, got {x}");
    assert_eq!(placed.placements[0].expect("the logo").x, 0.0);
    let version = placed.placements[index_of(&spec, "version")].expect("the version");
    assert!((version.right() - (3840.0 - 138.0)).abs() < EPS, "the version ends at the caption buttons");
}

/// As the bar narrows the version goes first, then the pills, then the Report a bug label shrinks.
#[test]
fn items_give_way_in_priority_order() {
    let (mut version_at, mut pills_at, mut report_label_at) = (0, 0, 0);
    let mut previous: Vec<String> = Vec::new();
    for width in (200..=3840).rev() {
        let spec = default_spec(width as f32);
        let placed = layout(&spec);
        let now = placed.overflow.clone();
        assert!(now.starts_with(&previous), "the overflow only grows as the bar narrows ({width}): {previous:?} then {now:?}");
        previous = now.clone();
        if version_at == 0 && now.iter().any(|id| id == "version") {
            version_at = width;
        }
        if pills_at == 0 && now.iter().any(|id| id == "donate" || id == "discord") {
            pills_at = width;
        }
        if report_label_at == 0 && placed.placements[index_of(&spec, "report")].is_some_and(|p| p.step > 0) {
            report_label_at = width;
        }
    }
    assert!(version_at > pills_at, "the version leaves at {version_at}, the pills at {pills_at}");
    assert!(pills_at > report_label_at, "the pills leave at {pills_at}, the report label at {report_label_at}");
    assert!(report_label_at > 0, "the report label does collapse on a narrow bar");
    assert_eq!(layout(&default_spec(version_at as f32)).overflow, ["version"], "the version is the first to go");
}

/// Locked items and the tabs' least width are kept at every width where they fit at all.
#[test]
fn locked_items_and_the_tabs_minimum_are_always_kept() {
    for width in 480..=3840 {
        let spec = default_spec(width as f32);
        let placed = layout(&spec);
        for item in spec.items.iter().filter(|i| i.locked) {
            assert!(placed.placements[index_of(&spec, &item.id)].is_some(), "{} is kept at {width}", item.id);
            assert!(!placed.overflow.contains(&item.id));
        }
        let (_, w) = placed.centre.expect("the tabs are kept");
        assert!(w >= default_bar::TABS_MIN - EPS, "the tabs keep their least at {width}, got {w}");
    }
}

/// Hostile specs never panic and never overlap: NaN, infinite and negative widths, an empty list,
/// repeated ids, empty ladders, a bar too narrow for its locked items and ten thousand items.
#[test]
fn hostile_specs_never_panic_or_overlap() {
    for width in 0..200 {
        let spec = default_spec(width as f32);
        assert_sound(&spec, &layout(&spec));
    }
    let nasty = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 0.0, 1.0e30, -1.0e30, 40.0, f32::MIN_POSITIVE];
    for &w in &nasty {
        for &c in &nasty {
            let mut spec = default_spec(w);
            spec.captions_left = c;
            spec.captions_right = c;
            spec.gap = c;
            spec.overflow_button_width = w;
            assert_sound(&spec, &layout(&spec));
            for item in &mut spec.items {
                item.ladder = vec![w, c, w];
            }
            assert_sound(&spec, &layout(&spec));
        }
    }
    let empty =
        Spec { width: 800.0, captions_left: 0.0, captions_right: 0.0, gap: 4.0, overflow_button_width: 30.0, items: Vec::new(), application: None };
    let placed = layout(&empty);
    assert!(placed.placements.is_empty() && placed.centre.is_none() && placed.overflow.is_empty());
    // A repeated id and a second centre item are ignored; an empty ladder is a zero width.
    let mut spec = default_spec(1000.0);
    spec.items.push(Item::new("save", Zone::End, &[34.0], 1));
    spec.items.push(Item::new("other-tabs", Zone::Centre, &[50.0], 1));
    spec.items.push(Item::new("no-ladder", Zone::Start, &[], 1));
    let placed = layout(&spec);
    assert_sound(&spec, &placed);
    let n = spec.items.len();
    assert!(placed.placements[n - 3].is_none() && placed.placements[n - 2].is_none());
    // Ten thousand items in every zone: past the cap nothing is placed.
    for width in [0.0, 100.0, 1000.0, 5000.0, 1.0e9] {
        let items = (0..10_000)
            .map(|i| {
                let zone = [Zone::Start, Zone::Centre, Zone::End][i % 3];
                Item::new(&format!("item{i}"), zone, &[30.0 + (i % 7) as f32, 20.0], (i % 251) as u8)
            })
            .collect();
        let spec = Spec { width, captions_left: 10.0, captions_right: 10.0, gap: 2.0, overflow_button_width: 24.0, items, application: None };
        let placed = layout(&spec);
        assert_sound(&spec, &placed);
        assert!(placed.placements.iter().skip(MAX_ITEMS).all(|p| p.is_none()));
    }
}

/// A small deterministic generator (an LCG) drives random specs: all sound, all repeatable.
#[test]
fn random_specs_never_overlap() {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move |n: u64| {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    for round in 0..2000 {
        let count = next(20) as usize;
        let items = (0..count)
            .map(|i| {
                let zone = [Zone::Start, Zone::Centre, Zone::End][next(3) as usize];
                let ladder: Vec<f32> = (0..next(4)).map(|_| next(200) as f32).collect();
                let id = format!("i{}", if next(10) == 0 { 0 } else { i });
                let mut item = Item::new(&id, zone, &ladder, next(256) as u8);
                item.locked = next(4) == 0;
                item.flexible = next(2) == 0;
                item
            })
            .collect();
        let spec = Spec {
            width: next(1500) as f32,
            captions_left: next(150) as f32,
            captions_right: next(150) as f32,
            gap: next(10) as f32,
            overflow_button_width: next(60) as f32,
            items,
            application: None,
        };
        let placed = layout(&spec);
        assert_sound(&spec, &placed);
        assert_eq!(placed, layout(&spec), "round {round} is deterministic");
    }
}

/// The same spec always gives the same answer, and equal priorities give way in source order.
#[test]
fn layout_is_deterministic() {
    for width in [320.0, 640.0, 900.0, 1280.0, 1920.0] {
        let spec = default_spec(width);
        assert_eq!(layout(&spec), layout(&spec.clone()));
    }
    let mut spec =
        Spec { width: 100.0, captions_left: 0.0, captions_right: 0.0, gap: 0.0, overflow_button_width: 10.0, items: Vec::new(), application: None };
    for id in ["a", "b", "c", "d"] {
        spec.items.push(Item::new(id, Zone::Start, &[40.0], 5));
    }
    let placed = layout(&spec);
    assert_eq!(placed.overflow, ["a", "b"], "equal priorities leave in source order");
    assert!(placed.placements[2].is_some() && placed.placements[3].is_some());
}

/// Exercise the newly projected pill targets and app-owned leading/search/assistant spaces
/// through real egui input. Existing bar tests cover the other action variants.
#[test]
fn app_bar_projected_pills_return_clicks_and_hide_clicks() {
    let ctx = crate::test_support::context();
    let bar = AppBar {
        items: None,
        quick_ids: &[],
        mark: &|_, _| {},
        name: "App",
        leading: Some(vec2(24.0, 24.0)),
        home: button("home", true, false),
        file: "File",
        file_open: false,
        actions: vec![button("save", true, false)],
        quick: 5,
        help: button("help", true, false),
        settings: button("settings", true, false),
        report: Report { label: "Report a bug", tip: "Report", icon_only: false },
        pills: ["donate", "discord"]
            .into_iter()
            .map(|id| Pill {
                id,
                icon: "help",
                label: id,
                labelled: true,
                tip: id,
                hide_tip: "Hide",
                name: id,
                hide_name: "Hide",
                fill: egui::Color32::WHITE,
                ink: egui::Color32::BLACK,
            })
            .collect(),
        search: true,
        search_shortcut: Some("Ctrl+K"),
        assistant: true,
        version: None,
        handles: HANDLES,
    };
    let time = std::cell::Cell::new(0.0);
    let frame = |events| {
        solidcraft_ui_registry::clear_handles();
        time.set(time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 400.0))),
            time: Some(time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let mut actions = Vec::new();
        crate::test_support::raw_frame(&ctx, input, |ui| {
            let r = Rect::from_min_size(pos2(17.25, 0.0), vec2(1800.0, HEIGHT));
            let out = show(ui, &bar, r, r.shrink2(vec2(78.0, 0.0)));
            assert!(out.leading.is_some() && out.search.is_some() && out.assistant.is_some());
            actions = out.actions;
        });
        actions
    };
    frame(Vec::new());
    for id in ["donate", "discord"] {
        for hide in [false, true] {
            let name = format!("t:{id}{}", if hide { ":hide" } else { "" });
            let at = solidcraft_ui_registry::handle_at(&name).expect("pill target");
            frame(vec![egui::Event::PointerMoved(at)]);
            frame(vec![press(at, true)]);
            let actions = frame(vec![press(at, false)]);
            assert_eq!(actions, vec![if hide { BarAction::HidePill { id: id.into() } } else { BarAction::Pill { id: id.into() } }]);
        }
    }
}

fn measured_bar() -> crate::app_bar_layout::application::Measurements {
    crate::app_bar_layout::application::Measurements {
        origin: 0.0,
        name: 50.0,
        file: 40.0,
        leading: Some(24.0),
        actions: vec![false, true, true],
        quick: 5,
        report: 60.0,
        report_icon_only: false,
        pills: vec![45.0, 45.0],
        search: Some((160.0, 42.0)),
        assistant: true,
        version: Some(30.0),
        offered: Some(80.0),
        managed: None,
        pixels_per_point: 1.0,
    }
}

/// Closed-form slot arithmetic: Home follows 108 + (24 + 8), File follows Home + 11,
/// actions step 34, and the quick group takes 8 + 5 * 34. The end slots total
/// 60 + 100 + 34 + 34 + 164 + 40 + 84 + 102 before the 138 caption points.
#[test]
fn app_bar_measured_groups_project_their_controls() {
    use crate::app_bar_layout::application::{Span, spec};
    let spec = spec(1920.0, 0.0, 138.0, measured_bar());
    assert_eq!(spec.items.iter().filter(|i| i.id == "quick").count(), 1);
    assert_eq!(spec.items.iter().filter(|i| i.id == "pills").count(), 1);
    assert!(spec.items.iter().any(|i| i.id == "tabs" && i.zone == Zone::Centre && i.flexible));
    let g = layout(&spec).application.expect("application geometry");
    assert_eq!(g.leading, Some(Span { left: 116.0, right: 140.0 }));
    assert_eq!(g.home, Span { left: 140.0, right: 172.0 });
    assert_eq!(g.brand_separator, 180.0);
    assert_eq!(g.file, Span { left: 183.0, right: 223.0 });
    assert_eq!(g.actions, vec![Some(240.0), Some(274.0), Some(308.0)]);
    assert_eq!(g.quick_separator, Some(333.0));
    assert_eq!(g.quick, Some((350.0, 34.0)));
    assert_eq!(g.documents_separator, 510.0);
    assert_eq!(g.tabs_start, 520.0);
    assert_eq!(g.controls_left, 1164.0);
    assert_eq!(g.offer, Span { left: 520.0, right: 1348.0 });
    assert!(g.offer_labeled && g.report_labeled);
    assert_eq!(g.report, Span { left: 1622.0, right: 1722.0 });
    assert_eq!(g.help, 1604.0);
    assert_eq!(g.settings, Span { left: 1554.0, right: 1586.0 });
    assert_eq!(g.search, Some(Span { left: 1390.0, right: 1550.0 }));
    assert_eq!(g.assistant, Some(Span { left: 1356.0, right: 1388.0 }));
    assert_eq!(g.version, Some(Span { left: 1740.0, right: 1770.0 }));
    assert_eq!(g.pills, vec![Span { left: 1166.0, right: 1211.0 }, Span { left: 1217.0, right: 1262.0 }]);
    // The offer drawn at its right edge starts at 1348 - 80 = 1268: six points
    // after the last link pill, with no overlap.
    assert_eq!(g.offer.right - 80.0 - g.pills.last().expect("last pill").right, 6.0);
}

/// Reserving transients without link pills must keep the same wide-bar anchors too.
#[test]
fn app_bar_wide_transients_keep_the_established_positions_without_links() {
    use crate::app_bar_layout::application::{Span, spec};
    let mut m = measured_bar();
    m.pills.clear();
    m.managed = Some(120.0);
    let p = layout(&spec(1920.0, 0.0, 138.0, m));
    assert!(p.overflow.is_empty());
    let g = p.application.expect("geometry");
    assert!(!g.reserved_offers, "the complete row keeps its original drawing geometry");
    assert_eq!(g.tabs_start, 520.0);
    assert_eq!(g.controls_left, 1350.0);
    assert_eq!(g.offer.right, 1348.0);
    assert_eq!(g.managed, Some(Span { left: 520.0, right: 1262.0 }));
}

#[test]
fn app_bar_measured_pills_snap_in_window_coordinates() {
    use crate::app_bar_layout::application::spec;
    let mut m = measured_bar();
    m.origin = 17.25;
    m.pixels_per_point = 2.0;
    m.pills = vec![45.3, 56.7];
    let g = layout(&spec(1920.0, 78.0, 0.0, m)).application.expect("geometry");
    for p in g.pills {
        for edge in [p.left, p.right] {
            let pixel = (edge + 17.25) * 2.0;
            assert_eq!(pixel, pixel.round());
        }
    }
}

/// Hostile measurements produce bounded work, finite drawing coordinates and repeatable results.
#[test]
fn app_bar_hostile_measurements_are_finite_and_bounded() {
    use crate::app_bar_layout::application::spec;
    let nasty = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 0.0, f32::MAX, f32::MIN_POSITIVE, 640.0];
    for &width in &nasty {
        for &v in &nasty {
            let mut m = measured_bar();
            m.origin = v;
            m.name = v;
            m.file = v;
            m.leading = Some(v);
            m.actions = vec![false; 10_000];
            m.quick = usize::MAX;
            m.report = v;
            m.pills = vec![v; 10_000];
            m.search = Some((v, v));
            m.version = Some(v);
            m.offered = Some(v);
            m.pixels_per_point = v;
            let spec = spec(width, v, v, m);
            assert!(spec.items.len() <= MAX_ITEMS);
            let p = layout(&spec);
            assert_eq!(p, layout(&spec));
            for p in p.placements.iter().flatten() {
                assert!(p.x.is_finite() && p.width.is_finite() && p.width >= 0.0);
            }
            let g = p.application.expect("geometry");
            assert!(g.actions.len() <= MAX_ITEMS);
            for x in [g.mark, g.name, g.brand_separator, g.documents_separator, g.tabs_start, g.controls_left, g.help] {
                assert!(x.is_finite());
            }
            let spans = [Some(g.home), Some(g.file), Some(g.report), Some(g.settings), g.leading, g.search, g.assistant, g.version];
            for s in spans.into_iter().flatten().chain(g.pills) {
                assert!(s.left.is_finite() && s.right.is_finite() && s.right >= s.left, "{s:?}");
            }
        }
    }
}

/// Every combination of the real bar's options, including transient reservations, uses the
/// same no-overlap contract as the plain engine. Check each integer width, including captions.
#[test]
fn app_bar_application_never_overlaps_at_any_width() {
    for left in [0.0, 78.0] {
        for quick in [0, 5, 24] {
            for bits in 0..512 {
                for width in 200..=3840 {
                    let has = |bit| bits & (1 << bit) != 0;
                    let mut m = measured_bar();
                    m.name = if has(6) { 380.0 } else { 50.0 };
                    m.quick = quick;
                    m.search = has(0).then_some((160.0, 42.0));
                    m.report_icon_only = has(1);
                    m.version = has(2).then_some(30.0);
                    m.pills = if has(3) { vec![100.0, 106.0] } else { Vec::new() };
                    m.assistant = has(4);
                    m.offered = has(5).then_some(110.0);
                    m.managed = has(8).then_some(160.0);
                    let spec = crate::app_bar_layout::application::spec(width as f32, left, if has(7) { 138.0 } else { 0.0 }, m);
                    let placed = layout(&spec);
                    assert_sound(&spec, &placed);
                    for item in spec.items.iter().filter(|i| i.locked) {
                        assert!(!placed.overflow.contains(&item.id), "locked {} at {width}/{bits}/{quick}", item.id);
                        if spec.width - spec.captions_left - spec.captions_right >= 232.0 {
                            assert!(placed.placements[index_of(&spec, &item.id)].is_some(), "locked {} fits at {width}/{bits}/{quick}", item.id);
                        }
                    }
                    let g = placed.application.as_ref().expect("geometry");
                    assert!(g.quick_count <= quick);
                    // Actual painted slots must stay inside their corresponding engine allocation.
                    for (id, span) in [("search", g.search), ("assistant", g.assistant), ("managed", g.managed)] {
                        if let Some(span) = span.filter(|_| id != "managed" || g.reserved_offers) {
                            let slot = placed.placements[index_of(&spec, id)].expect("visible slot");
                            assert!(span.left >= slot.x - EPS && span.right <= slot.right() + EPS);
                        }
                    }
                    if placed.overflow.iter().any(|id| id == "offered") {
                        assert_eq!(g.offer.left, g.offer.right, "hidden update cannot paint over tabs");
                    }
                }
            }
        }
    }
}

#[test]
fn app_bar_overflow_follows_bar_order_and_quick_buttons_leave_from_the_end() {
    let mut m = measured_bar();
    m.quick = 24;
    let spec = crate::app_bar_layout::application::spec(400.0, 0.0, 0.0, m);
    let p = layout(&spec);
    let indices: Vec<_> = p.overflow.iter().map(|id| index_of(&spec, id)).collect();
    assert!(indices.windows(2).all(|p| p[0] < p[1]));
    let mut m = measured_bar();
    m.quick = 24;
    let p = layout(&crate::app_bar_layout::application::spec(1100.0, 0.0, 0.0, m));
    let g = p.application.expect("geometry");
    assert!(g.quick_count > 0 && g.quick_count < 24, "this case retains a prefix of quick buttons");
    for i in g.quick_count..24 {
        assert!(p.overflow.contains(&format!("quick:{i}")));
    }
}

#[test]
fn app_bar_more_menu_returns_the_original_action_and_closes() {
    let rig = BarRig::new(250.0);
    rig.click("appbar:overflow");
    rig.frame(Vec::new());
    let rows = rig.out.borrow().as_ref().expect("bar").overflow_items.clone();
    let report = rows.iter().find(|r| r.id == "report").expect("report overflowed");
    assert_eq!(report.action, BarAction::Report);
    assert_eq!(rig.click("menu:appbar:overflow:report"), [BarAction::Report]);
    assert!(!rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
    rig.click("appbar:overflow");
    rig.frame(Vec::new());
    assert!(rig.click("menu:appbar:overflow:action:1").is_empty(), "disabled undo cannot run");
    assert!(rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
    rig.frame(vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    assert!(!rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
    rig.click("appbar:overflow");
    rig.frame(Vec::new());
    let at = pos2(240.0, 390.0);
    for pressed in [true, false] {
        rig.frame(vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]);
    }
    assert!(!rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
}

mod app_bar_items {
    use crate::app_bar_items::{ItemId, Layout, Placement, Zone};
    use serde_json::json;

    #[test]
    fn defaults_match_the_actual_drawing_order() {
        let layout = Layout::default_for(["solid.box", "solid.fillet"]);
        let ids = |zone| layout.items.iter().filter(|p| p.zone == zone).map(|p| p.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids(Zone::Start), ["home", "save", "undo", "redo", "quick:solid.box", "quick:solid.fillet"]);
        assert_eq!(ids(Zone::End), ["donate", "discord", "assistant", "search", "settings", "help", "report", "version"]);
        assert!(layout.items.iter().all(|p| p.shown));
        for item in &layout.items {
            assert_eq!(ItemId::parse(&item.id).unwrap().id(), item.id);
        }
        assert_eq!(Layout::default_for(["", "\0", &"x".repeat(95)]), Layout::default());
    }

    #[test]
    fn sanitize_keeps_unknowns_and_first_placement_and_restores_missing_items() {
        let defaults = Layout::default();
        let mut layout = Layout {
            items: vec![
                Placement { id: "save".into(), zone: Zone::End, shown: false },
                Placement { id: "future:item".into(), zone: Zone::Start, shown: true },
                Placement { id: "save".into(), zone: Zone::Start, shown: true },
            ],
        };
        layout.sanitize(&defaults);
        assert_eq!(layout.items.iter().filter(|p| p.id == "save").count(), 1);
        assert!(layout.items.iter().any(|p| p.id == "save" && p.zone == Zone::End && !p.shown));
        assert!(layout.items.iter().any(|p| p.id == "future:item"));
        assert!(defaults.items.iter().all(|d| layout.items.iter().any(|p| p.id == d.id)));
        let once = layout.clone();
        layout.sanitize(&defaults);
        assert_eq!(layout, once);
    }

    #[test]
    fn hostile_rows_are_bounded_without_starving_known_items() {
        let defaults = Layout::default();
        let mut layout = Layout { items: (0..10_000).map(|i| Placement { id: format!("future:{i}"), zone: Zone::Start, shown: true }).collect() };
        layout.items.splice(
            0..0,
            ["".to_string(), "a".repeat(101), "\0home".to_string(), "🦀".repeat(100)].map(|id| Placement { id, zone: Zone::End, shown: true }),
        );
        layout.sanitize(&defaults);
        assert_eq!(layout.items.len(), 64);
        assert!(defaults.items.iter().all(|d| layout.items.iter().any(|p| p.id == d.id)));
        assert!(layout.items.iter().any(|p| p.id == "🦀".repeat(100)));
        assert!(layout.items.iter().all(|p| !p.id.is_empty() && p.id.chars().count() <= 100 && !p.id.chars().any(char::is_control)));
    }

    #[test]
    fn json_round_trip_and_malformed_rows() {
        let defaults = Layout::default_for(["solid.box"]);
        let mut layout = defaults.clone();
        layout.items.reverse();
        layout.items[0].shown = false;
        layout.items.push(Placement { id: "future".into(), zone: Zone::End, shown: false });
        assert_eq!(Layout::from_value(&layout.to_value(), &defaults), layout);
        for value in [
            json!(null),
            json!([]),
            json!({"items":[7, {}, {"id":"save","zone":"centre","shown":true}, {"id":"update","zone":"start","shown":false}, {"id":"file","zone":"end","shown":false}]}),
        ] {
            assert_eq!(Layout::from_value(&value, &defaults), defaults);
        }
    }
}

#[test]
fn app_bar_saved_defaults_use_the_unchanged_geometry_path() {
    use crate::app_bar_layout::application::{spec, spec_with_items};
    let quick = ["a", "b", "c", "d", "e"];
    let defaults = crate::app_bar_items::Layout::default_for(quick);
    for width in [200.0, 480.0, 640.0, 1280.0, 1920.0, 3840.0] {
        assert_eq!(
            spec(width, 0.0, 138.0, measured_bar()),
            spec_with_items((width, 0.0, 138.0), measured_bar(), &defaults, &["save", "undo", "redo"], &quick, &["donate", "discord"])
        );
    }
}

#[test]
fn app_bar_saved_items_move_hide_and_overflow_independently() {
    use crate::app_bar_items::{Layout, Zone as SavedZone};
    use crate::app_bar_layout::application::spec_with_items;
    let quick = ["a", "b", "c", "d", "e"];
    let mut saved = Layout::default_for(quick);
    saved.items.iter_mut().find(|p| p.id == "version").unwrap().shown = false;
    let minor = spec_with_items((3840.0, 0.0, 0.0), measured_bar(), &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
    assert!(index_of(&minor, "home") < index_of(&minor, "file"));
    assert!(index_of(&minor, "pill:1") < index_of(&minor, "offered"));
    assert!(index_of(&minor, "offered") < index_of(&minor, "assistant"));
    saved.items.reverse();
    for p in &mut saved.items {
        if p.id == "undo" {
            p.shown = false;
        }
        if p.id == "quick:a" || p.id == "save" {
            p.zone = SavedZone::End;
        }
    }
    let spec = |width| spec_with_items((width, 0.0, 0.0), measured_bar(), &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
    let wide = spec(3840.0);
    let placed = layout(&wide);
    let g = placed.application.as_ref().unwrap();
    assert!(g.actions[1].is_none());
    assert!(g.actions[0].unwrap() > g.tabs_end);
    assert_eq!(g.quick_slots.len(), 5);
    assert!(g.quick_slots.iter().find(|(i, _)| *i == 0).unwrap().1 > g.tabs_end);
    assert!(g.quick_slots.iter().find(|(i, _)| *i == 4).unwrap().1 < g.quick_slots.iter().find(|(i, _)| *i == 3).unwrap().1);
    let narrow = spec(300.0);
    let placed = layout(&narrow);
    assert!(!placed.overflow.is_empty());
    assert!(!narrow.items.iter().any(|i| i.id == "action:1"));
    assert!(!placed.overflow.iter().any(|i| i == "action:1"));
    assert_sound(&narrow, &placed);
}

#[test]
fn app_bar_random_sanitized_layouts_never_overlap() {
    use crate::app_bar_items::{Layout, Placement, Zone as SavedZone};
    use crate::app_bar_layout::application::spec_with_items;
    let quick = ["a", "b", "c", "d", "e"];
    let defaults = Layout::default_for(quick);
    let mut seed = 0x19404_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed >> 32
    };
    for _ in 0..16 {
        let mut saved = defaults.clone();
        for i in (1..saved.items.len()).rev() {
            let j = random() as usize % (i + 1);
            saved.items.swap(i, j);
        }
        for p in &mut saved.items {
            p.zone = if random() & 1 == 0 { SavedZone::Start } else { SavedZone::End };
            p.shown = !random().is_multiple_of(3);
        }
        saved.items.push(Placement { id: "future:item".into(), zone: SavedZone::End, shown: true });
        saved.items.push(saved.items[0].clone());
        saved.items.remove(random() as usize % saved.items.len());
        saved.sanitize(&defaults);
        for width in 200..=3840 {
            let mut measurements = measured_bar();
            measurements.managed = Some(130.0);
            let spec = spec_with_items((width as f32, 78.0, 138.0), measurements, &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
            let placed = layout(&spec);
            assert_sound(&spec, &placed);
            let g = placed.application.unwrap();
            for (i, x) in &g.quick_slots {
                let slot = placed.placements[index_of(&spec, &format!("quick:{i}"))].unwrap();
                assert!(*x - 16.0 >= slot.x - EPS && *x + 16.0 <= slot.right() + EPS);
            }
        }
    }
}

// Unchanged rows are literal old-show snapshots. Revised rows are derived from the old
// measured label widths and packed 34-point actions, 124-point tabs and 32-point More,
// conceding by priority. See app_bar_application_never_overlaps_at_any_width for the contract.
// Each case stores field length followed by its millipoint values.
// Rows follow the width/variant loop order; zero lengths preserve absent rects and centres.
#[rustfmt::skip]
const OLD_BAR_ROWS: &[&[i32]] = &[
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 380000, 4000, 380000, 30000, 1, 0, 2, 206281, 380000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 0
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 4000, 348000, 30000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 1
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 79000, 4000, 115281, 30000, 2, 97141, 17000, 4, 116281, 1000, 148281, 33000, 2, 132281, 17000, 4, 150281, 1000, 182281, 33000, 2, 166281, 17000, 4, 184281, 1000, 216281, 33000, 2, 200281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 4, 75500, 7000, 76500, 27000, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 1000, 480000, 33000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 380000, 1000, 380000, 33000, 1, 0, 2, 217281, 380000, 0, 0, 0, 4, 79000, 4000, 115281, 30000], // 480 / 2
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 4000, 348000, 30000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 4
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 4000, 348000, 30000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 8
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 340000, 4000, 340000, 30000, 1, 0, 2, 206281, 340000, 0, 4, 346000, 1000, 378000, 33000, 0, 4, 68000, 4000, 104281, 30000], // 480 / 16
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 446000, 1000, 478000, 33000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 412000, 1000, 444000, 33000, 2, 428000, 17000, 0, 0, 2, 223281, 34000, 4, 380000, 4000, 380000, 30000, 1, 0, 2, 240281, 380000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 32
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 380000, 4000, 380000, 30000, 1, 0, 2, 206281, 380000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 64
    &[0, 0, 4, 36000, 4000, 72281, 30000, 2, 54141, 17000, 4, 73281, 1000, 105281, 33000, 2, 89281, 17000, 0, 0, 0, 0, 4, 308000, 1000, 340000, 33000, 2, 324000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 274000, 1000, 306000, 33000, 2, 290000, 17000, 0, 0, 0, 4, 242000, 4000, 242000, 30000, 1, 0, 2, 106281, 242000, 0, 0, 0, 4, 36000, 4000, 72281, 30000], // 480 / 128
    &[0, 0, 4, 36000, 4000, 72281, 30000, 2, 54141, 17000, 4, 73281, 1000, 105281, 33000, 2, 89281, 17000, 0, 0, 0, 0, 4, 308000, 1000, 340000, 33000, 2, 324000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 274000, 1000, 306000, 33000, 2, 290000, 17000, 0, 0, 0, 4, 242000, 1000, 242000, 33000, 1, 0, 2, 106281, 242000, 0, 0, 0, 4, 36000, 4000, 72281, 30000], // 480 / 255
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 446000, 1000, 478000, 33000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 412000, 1000, 444000, 33000, 2, 428000, 17000, 0, 0, 0, 4, 340000, 4000, 340000, 30000, 1, 0, 2, 206281, 340000, 0, 4, 378000, 1000, 410000, 33000, 0, 4, 68000, 4000, 104281, 30000], // 480 / 28
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 4, 342000, 4000, 376000, 30000, 2, 359000, 17000, 4, 448000, 1000, 480000, 33000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 342000, 1000, 342000, 33000, 1, 0, 2, 206281, 342000, 4, 342000, 4000, 376000, 30000, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 3
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 4000, 348000, 30000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 5
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 1000, 480000, 33000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 1000, 348000, 33000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 6
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 4000, 348000, 30000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 9
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 446000, 1000, 478000, 33000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 412000, 1000, 444000, 33000, 2, 428000, 17000, 0, 0, 0, 4, 340000, 4000, 340000, 30000, 1, 0, 2, 206281, 340000, 0, 4, 378000, 1000, 410000, 33000, 0, 4, 68000, 4000, 104281, 30000], // 480 / 17
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 446000, 1000, 478000, 33000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 412000, 1000, 444000, 33000, 2, 428000, 17000, 0, 0, 0, 4, 340000, 4000, 340000, 30000, 1, 0, 2, 206281, 340000, 0, 4, 378000, 1000, 410000, 33000, 0, 4, 68000, 4000, 104281, 30000], // 480 / 24
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 446000, 1000, 478000, 33000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 412000, 1000, 444000, 33000, 2, 428000, 17000, 0, 0, 2, 223281, 34000, 4, 380000, 4000, 380000, 30000, 1, 0, 2, 240281, 380000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 33
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 414000, 1000, 446000, 33000, 2, 430000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 448000, 4000, 480000, 30000, 2, 464000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 380000, 1000, 412000, 33000, 2, 396000, 17000, 0, 0, 0, 4, 348000, 4000, 348000, 30000, 1, 0, 2, 206281, 348000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 65
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 446000, 1000, 478000, 33000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 412000, 1000, 444000, 33000, 2, 428000, 17000, 0, 0, 2, 223281, 34000, 4, 380000, 4000, 380000, 30000, 1, 0, 2, 240281, 380000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 480 / 96
    &[0, 0, 4, 36000, 4000, 72281, 30000, 2, 54141, 17000, 4, 73281, 1000, 105281, 33000, 2, 89281, 17000, 0, 0, 0, 0, 4, 308000, 1000, 340000, 33000, 2, 324000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 274000, 1000, 306000, 33000, 2, 290000, 17000, 0, 0, 0, 4, 242000, 4000, 242000, 30000, 1, 0, 2, 106281, 242000, 0, 0, 0, 4, 36000, 4000, 72281, 30000], // 480 / 129
    &[0, 0, 4, 36000, 4000, 72281, 30000, 2, 54141, 17000, 4, 73281, 1000, 105281, 33000, 2, 89281, 17000, 0, 0, 0, 0, 4, 308000, 1000, 340000, 33000, 2, 324000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 274000, 1000, 306000, 33000, 2, 290000, 17000, 0, 0, 0, 4, 242000, 4000, 242000, 30000, 1, 0, 2, 106281, 242000, 0, 0, 0, 4, 36000, 4000, 72281, 30000], // 480 / 160
    &[0, 0, 4, 36000, 4000, 72281, 30000, 2, 54141, 17000, 4, 73281, 1000, 105281, 33000, 2, 89281, 17000, 0, 0, 0, 0, 4, 308000, 1000, 340000, 33000, 2, 324000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 274000, 1000, 306000, 33000, 2, 290000, 17000, 0, 0, 0, 4, 242000, 4000, 242000, 30000, 1, 0, 2, 106281, 242000, 0, 0, 0, 4, 36000, 4000, 72281, 30000], // 480 / 188
    &[0, 0, 4, 36000, 4000, 72281, 30000, 2, 54141, 17000, 4, 73281, 1000, 105281, 33000, 2, 89281, 17000, 0, 0, 0, 0, 4, 308000, 1000, 340000, 33000, 2, 324000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 274000, 1000, 306000, 33000, 2, 290000, 17000, 0, 0, 0, 4, 242000, 1000, 242000, 33000, 1, 0, 2, 106281, 242000, 0, 0, 0, 4, 36000, 4000, 72281, 30000], // 480 / 254
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 570000, 1000, 602000, 33000, 2, 586000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 604000, 4000, 632000, 30000, 2, 618000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 536000, 1000, 568000, 33000, 2, 552000, 17000, 0, 0, 0, 4, 306281, 4000, 494000, 30000, 1, 0, 2, 306281, 496000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 640 / 0
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 570000, 1000, 602000, 33000, 2, 586000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 498000, 4000, 532000, 30000, 2, 515000, 17000, 4, 604000, 4000, 632000, 30000, 2, 618000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 536000, 1000, 568000, 33000, 2, 552000, 17000, 0, 0, 0, 4, 306281, 4000, 456000, 30000, 1, 0, 2, 306281, 458000, 4, 498000, 4000, 532000, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 640 / 1
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 566000, 1000, 598000, 33000, 2, 582000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 600000, 1000, 632000, 33000, 2, 616000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 532000, 1000, 564000, 33000, 2, 548000, 17000, 0, 0, 0, 4, 306281, 1000, 490000, 33000, 1, 1, 2, 306281, 492000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 640 / 2
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 518000, 1000, 550000, 33000, 2, 534000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 552000, 4000, 580000, 30000, 2, 566000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 484000, 1000, 516000, 33000, 2, 500000, 17000, 0, 2, 617797, 17000, 0, 4, 306281, 4000, 442000, 30000, 1, 0, 2, 306281, 444000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 640 / 4
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 79000, 4000, 115281, 30000, 2, 97141, 17000, 4, 116281, 1000, 148281, 33000, 2, 132281, 17000, 4, 150281, 1000, 182281, 33000, 2, 166281, 17000, 4, 184281, 1000, 216281, 33000, 2, 200281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 4, 75500, 7000, 76500, 27000, 0, 0, 0, 0, 0, 0, 0, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 4, 354844, 5000, 380000, 29000, 2, 367422, 17000, 4, 380000, 9000, 396000, 25000, 2, 388000, 17000, 4, 405844, 5000, 431000, 29000, 2, 418422, 17000, 4, 431000, 9000, 447000, 25000, 2, 439000, 17000, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 354844, 4000, 354844, 30000, 1, 0, 2, 217281, 354844, 0, 0, 0, 4, 79000, 4000, 115281, 30000], // 640 / 8
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 570000, 1000, 602000, 33000, 2, 586000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 604000, 4000, 632000, 30000, 2, 618000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 536000, 1000, 568000, 33000, 2, 552000, 17000, 0, 0, 0, 4, 306281, 4000, 494000, 30000, 1, 0, 2, 306281, 496000, 0, 4, 502000, 1000, 534000, 33000, 0, 4, 151000, 4000, 187281, 30000], // 640 / 16
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 574000, 1000, 606000, 33000, 2, 590000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 608000, 4000, 640000, 30000, 2, 624000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 540000, 1000, 572000, 33000, 2, 556000, 17000, 0, 0, 2, 223281, 34000, 4, 540000, 4000, 540000, 30000, 1, 0, 2, 376281, 540000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 32
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 79000, 4000, 115281, 30000, 2, 97141, 17000, 4, 116281, 1000, 148281, 33000, 2, 132281, 17000, 4, 150281, 1000, 182281, 33000, 2, 166281, 17000, 4, 184281, 1000, 216281, 33000, 2, 200281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 4, 75500, 7000, 76500, 27000, 0, 0, 0, 0, 0, 0, 0, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 456844, 4000, 456844, 30000, 1, 0, 2, 217281, 456844, 0, 0, 0, 4, 79000, 4000, 115281, 30000], // 640 / 64
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 436000, 1000, 468000, 33000, 2, 452000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 470000, 4000, 502000, 30000, 2, 486000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 402000, 1000, 434000, 33000, 2, 418000, 17000, 0, 0, 0, 4, 402000, 4000, 402000, 30000, 1, 0, 2, 206281, 402000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 128
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 468000, 1000, 500000, 33000, 2, 484000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 434000, 1000, 466000, 33000, 2, 450000, 17000, 0, 0, 2, 223281, 34000, 4, 402000, 1000, 402000, 33000, 1, 0, 2, 274281, 402000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 255
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 0, 0, 0, 0, 4, 365844, 5000, 391000, 29000, 2, 378422, 17000, 4, 391000, 9000, 407000, 25000, 2, 399000, 17000, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 333844, 4000, 333844, 30000, 1, 0, 2, 206281, 333844, 0, 4, 422844, 1000, 454844, 33000, 0, 4, 68000, 4000, 104281, 30000], // 640 / 28
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 566000, 1000, 598000, 33000, 2, 582000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 494000, 4000, 528000, 30000, 2, 511000, 17000, 4, 600000, 1000, 632000, 33000, 2, 616000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 532000, 1000, 564000, 33000, 2, 548000, 17000, 0, 0, 0, 4, 306281, 1000, 452000, 33000, 1, 1, 2, 306281, 454000, 4, 494000, 4000, 528000, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 640 / 3
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 0, 0, 0, 0, 0, 0, 4, 418844, 4000, 452844, 30000, 2, 435844, 17000, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 386844, 4000, 386844, 30000, 1, 0, 2, 206281, 386844, 4, 418844, 4000, 452844, 30000, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 5
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 514000, 1000, 546000, 33000, 2, 530000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 548000, 1000, 580000, 33000, 2, 564000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 480000, 1000, 512000, 33000, 2, 496000, 17000, 0, 2, 617797, 17000, 0, 4, 306281, 1000, 438000, 33000, 1, 1, 2, 306281, 440000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 640 / 6
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 0, 0, 0, 0, 0, 0, 4, 418844, 4000, 452844, 30000, 2, 435844, 17000, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 386844, 4000, 386844, 30000, 1, 0, 2, 206281, 386844, 4, 418844, 4000, 452844, 30000, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 9
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 570000, 1000, 602000, 33000, 2, 586000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 498000, 4000, 532000, 30000, 2, 515000, 17000, 4, 604000, 4000, 632000, 30000, 2, 618000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 536000, 1000, 568000, 33000, 2, 552000, 17000, 0, 0, 0, 4, 306281, 4000, 456000, 30000, 1, 0, 2, 306281, 458000, 4, 498000, 4000, 532000, 30000, 4, 464000, 1000, 496000, 33000, 0, 4, 151000, 4000, 187281, 30000], // 640 / 17
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 0, 0, 0, 0, 4, 365844, 5000, 391000, 29000, 2, 378422, 17000, 4, 391000, 9000, 407000, 25000, 2, 399000, 17000, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 333844, 4000, 333844, 30000, 1, 0, 2, 206281, 333844, 0, 4, 422844, 1000, 454844, 33000, 0, 4, 68000, 4000, 104281, 30000], // 640 / 24
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 574000, 1000, 606000, 33000, 2, 590000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 608000, 4000, 640000, 30000, 2, 624000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 540000, 1000, 572000, 33000, 2, 556000, 17000, 0, 0, 2, 223281, 34000, 4, 508000, 4000, 508000, 30000, 1, 0, 2, 376281, 508000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 33
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 490844, 1000, 522844, 33000, 2, 506844, 17000, 0, 0, 0, 0, 0, 0, 4, 418844, 4000, 452844, 30000, 2, 435844, 17000, 4, 524844, 4000, 640000, 30000, 2, 582422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456844, 1000, 488844, 33000, 2, 472844, 17000, 0, 0, 0, 4, 418844, 4000, 418844, 30000, 1, 0, 2, 206281, 418844, 4, 418844, 4000, 452844, 30000, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 65
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 574000, 1000, 606000, 33000, 2, 590000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 608000, 4000, 640000, 30000, 2, 624000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 540000, 1000, 572000, 33000, 2, 556000, 17000, 0, 0, 2, 223281, 34000, 4, 540000, 4000, 540000, 30000, 1, 0, 2, 376281, 540000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 96
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 436000, 1000, 468000, 33000, 2, 452000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 470000, 4000, 502000, 30000, 2, 486000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 402000, 1000, 434000, 33000, 2, 418000, 17000, 0, 0, 0, 4, 370000, 4000, 370000, 30000, 1, 0, 2, 206281, 370000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 129
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 468000, 1000, 500000, 33000, 2, 484000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 434000, 1000, 466000, 33000, 2, 450000, 17000, 0, 0, 2, 223281, 34000, 4, 402000, 4000, 402000, 30000, 1, 0, 2, 274281, 402000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 160
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 468000, 1000, 500000, 33000, 2, 484000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 434000, 1000, 466000, 33000, 2, 450000, 17000, 0, 0, 2, 223281, 34000, 4, 402000, 4000, 402000, 30000, 1, 0, 2, 274281, 402000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 188
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 468000, 1000, 500000, 33000, 2, 484000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 434000, 1000, 466000, 33000, 2, 450000, 17000, 0, 0, 2, 223281, 34000, 4, 402000, 1000, 402000, 33000, 1, 0, 2, 274281, 402000, 0, 0, 0, 4, 68000, 4000, 104281, 30000], // 640 / 254
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 306281, 4000, 766844, 30000, 1, 1, 2, 306281, 768844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 0
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 621844, 4000, 804844, 30000, 2, 713344, 17000, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 306281, 4000, 579844, 30000, 1, 1, 2, 306281, 581844, 4, 621844, 4000, 804844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 1
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 926000, 1000, 958000, 33000, 2, 942000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 960000, 1000, 992000, 33000, 2, 976000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 892000, 1000, 924000, 33000, 2, 908000, 17000, 0, 0, 0, 4, 306281, 1000, 850000, 33000, 1, 1, 2, 306281, 852000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 2
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 790844, 1000, 822844, 33000, 2, 806844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 824844, 4000, 940000, 30000, 2, 882422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 756844, 1000, 788844, 33000, 2, 772844, 17000, 0, 2, 977797, 17000, 0, 4, 306281, 4000, 714844, 30000, 1, 1, 2, 306281, 716844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 4
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 4, 669000, 5000, 694000, 29000, 2, 681500, 17000, 4, 694000, 9000, 710000, 25000, 2, 702000, 17000, 4, 720000, 5000, 745000, 29000, 2, 732500, 17000, 4, 745000, 9000, 761000, 25000, 2, 753000, 17000, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 306281, 4000, 766844, 30000, 1, 1, 2, 306281, 666844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 8
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 306281, 4000, 766844, 30000, 1, 1, 2, 306281, 768844, 0, 4, 774844, 1000, 806844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 16
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 766844, 30000, 1, 1, 2, 484281, 768844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 32
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 4, 443531, 7000, 444531, 27000, 0, 0, 0, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 454031, 4000, 766844, 30000, 1, 1, 2, 454031, 768844, 0, 0, 0, 4, 298750, 4000, 335031, 30000], // 1000 / 64
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 704844, 1000, 736844, 33000, 2, 720844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 738844, 4000, 854000, 30000, 2, 796422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 670844, 1000, 702844, 33000, 2, 686844, 17000, 0, 0, 0, 4, 306281, 4000, 628844, 30000, 1, 1, 2, 306281, 630844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 128
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 796000, 1000, 828000, 33000, 2, 812000, 17000, 0, 0, 0, 0, 0, 0, 4, 575000, 4000, 758000, 30000, 2, 666500, 17000, 4, 830000, 1000, 862000, 33000, 2, 846000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 762000, 1000, 794000, 33000, 2, 778000, 17000, 0, 0, 2, 223281, 34000, 4, 503000, 1000, 503000, 33000, 1, 0, 2, 376281, 503000, 4, 575000, 4000, 758000, 30000, 4, 541000, 1000, 573000, 33000, 0, 4, 68000, 4000, 104281, 30000], // 1000 / 255
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 790844, 1000, 822844, 33000, 2, 806844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 824844, 4000, 940000, 30000, 2, 882422, 17000, 4, 617000, 5000, 642000, 29000, 2, 629500, 17000, 4, 642000, 9000, 658000, 25000, 2, 650000, 17000, 4, 668000, 5000, 693000, 29000, 2, 680500, 17000, 4, 693000, 9000, 709000, 25000, 2, 701000, 17000, 4, 756844, 1000, 788844, 33000, 2, 772844, 17000, 0, 2, 977797, 17000, 0, 4, 306281, 4000, 714844, 30000, 1, 1, 2, 306281, 614844, 0, 4, 722844, 1000, 754844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 28
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 926000, 1000, 958000, 33000, 2, 942000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 705000, 4000, 888000, 30000, 2, 796500, 17000, 4, 960000, 1000, 992000, 33000, 2, 976000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 892000, 1000, 924000, 33000, 2, 908000, 17000, 0, 0, 0, 4, 306281, 1000, 663000, 33000, 1, 1, 2, 306281, 665000, 4, 705000, 4000, 888000, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 3
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 790844, 1000, 822844, 33000, 2, 806844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 569844, 4000, 752844, 30000, 2, 661344, 17000, 4, 824844, 4000, 940000, 30000, 2, 882422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 756844, 1000, 788844, 33000, 2, 772844, 17000, 0, 2, 977797, 17000, 0, 4, 306281, 4000, 527844, 30000, 1, 1, 2, 306281, 529844, 4, 569844, 4000, 752844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 5
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 874000, 1000, 906000, 33000, 2, 890000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 908000, 1000, 940000, 33000, 2, 924000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 840000, 1000, 872000, 33000, 2, 856000, 17000, 0, 2, 977797, 17000, 0, 4, 306281, 1000, 798000, 33000, 1, 1, 2, 306281, 800000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 6
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 850844, 1000, 882844, 33000, 2, 866844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 0, 0, 4, 629844, 4000, 812844, 30000, 2, 721344, 17000, 4, 884844, 4000, 1000000, 30000, 2, 942422, 17000, 4, 527844, 5000, 553000, 29000, 2, 540422, 17000, 4, 553000, 9000, 569000, 25000, 2, 561000, 17000, 4, 578844, 5000, 604000, 29000, 2, 591422, 17000, 4, 604000, 9000, 620000, 25000, 2, 612000, 17000, 4, 816844, 1000, 848844, 33000, 2, 832844, 17000, 0, 0, 0, 4, 527844, 4000, 527844, 30000, 1, 0, 2, 289281, 527844, 4, 629844, 4000, 812844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 9
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 621844, 4000, 804844, 30000, 2, 713344, 17000, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 306281, 4000, 579844, 30000, 1, 1, 2, 306281, 581844, 4, 621844, 4000, 804844, 30000, 4, 587844, 1000, 619844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 17
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 4, 669000, 5000, 694000, 29000, 2, 681500, 17000, 4, 694000, 9000, 710000, 25000, 2, 702000, 17000, 4, 720000, 5000, 745000, 29000, 2, 732500, 17000, 4, 745000, 9000, 761000, 25000, 2, 753000, 17000, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 306281, 4000, 766844, 30000, 1, 1, 2, 306281, 666844, 0, 4, 774844, 1000, 806844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 24
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 4, 770844, 4000, 804844, 30000, 2, 787844, 17000, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 728844, 30000, 1, 1, 2, 484281, 730844, 4, 770844, 4000, 804844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 33
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 842844, 1000, 874844, 33000, 2, 858844, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 4, 443531, 7000, 444531, 27000, 0, 4, 770844, 4000, 804844, 30000, 2, 787844, 17000, 4, 876844, 4000, 992000, 30000, 2, 934422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 808844, 1000, 840844, 33000, 2, 824844, 17000, 0, 0, 0, 4, 454031, 4000, 728844, 30000, 1, 1, 2, 454031, 730844, 4, 770844, 4000, 804844, 30000, 0, 0, 4, 298750, 4000, 335031, 30000], // 1000 / 65
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 930000, 1000, 962000, 33000, 2, 946000, 17000, 4, 295250, 7000, 296250, 27000, 0, 4, 444531, 7000, 445531, 27000, 0, 4, 621531, 7000, 622531, 27000, 0, 0, 0, 4, 964000, 4000, 992000, 30000, 2, 978000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 896000, 1000, 928000, 33000, 2, 912000, 17000, 0, 0, 2, 462031, 34000, 4, 632031, 4000, 854000, 30000, 1, 0, 2, 632031, 856000, 0, 0, 0, 4, 298750, 4000, 335031, 30000], // 1000 / 96
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 704844, 1000, 736844, 33000, 2, 720844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 632844, 4000, 666844, 30000, 2, 649844, 17000, 4, 738844, 4000, 854000, 30000, 2, 796422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 670844, 1000, 702844, 33000, 2, 686844, 17000, 0, 0, 0, 4, 306281, 4000, 590844, 30000, 1, 1, 2, 306281, 592844, 4, 632844, 4000, 666844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 129
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 792000, 1000, 824000, 33000, 2, 808000, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 826000, 4000, 854000, 30000, 2, 840000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 758000, 1000, 790000, 33000, 2, 774000, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 716000, 30000, 1, 0, 2, 484281, 718000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1000 / 160
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 68000, 4000, 104281, 30000, 2, 86141, 17000, 4, 105281, 1000, 137281, 33000, 2, 121281, 17000, 4, 139281, 1000, 171281, 33000, 2, 155281, 17000, 4, 173281, 1000, 205281, 33000, 2, 189281, 17000, 4, 712844, 1000, 744844, 33000, 2, 728844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 746844, 4000, 862000, 30000, 2, 804422, 17000, 4, 536844, 5000, 562000, 29000, 2, 549422, 17000, 4, 562000, 9000, 578000, 25000, 2, 570000, 17000, 4, 587844, 5000, 613000, 29000, 2, 600422, 17000, 4, 613000, 9000, 629000, 25000, 2, 621000, 17000, 4, 678844, 1000, 710844, 33000, 2, 694844, 17000, 0, 0, 2, 223281, 34000, 4, 504844, 4000, 504844, 30000, 1, 0, 2, 376281, 504844, 0, 4, 644844, 1000, 676844, 33000, 0, 4, 68000, 4000, 104281, 30000], // 1000 / 188
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 79000, 4000, 115281, 30000, 2, 97141, 17000, 4, 116281, 1000, 148281, 33000, 2, 132281, 17000, 4, 150281, 1000, 182281, 33000, 2, 166281, 17000, 4, 184281, 1000, 216281, 33000, 2, 200281, 17000, 4, 736000, 1000, 768000, 33000, 2, 752000, 17000, 4, 75500, 7000, 76500, 27000, 0, 0, 0, 0, 0, 0, 0, 4, 770000, 1000, 802000, 33000, 2, 786000, 17000, 4, 560000, 5000, 585000, 29000, 2, 572500, 17000, 4, 585000, 9000, 601000, 25000, 2, 593000, 17000, 4, 611000, 5000, 636000, 29000, 2, 623500, 17000, 4, 636000, 9000, 652000, 25000, 2, 644000, 17000, 4, 702000, 1000, 734000, 33000, 2, 718000, 17000, 0, 2, 839797, 17000, 2, 234281, 34000, 4, 560000, 1000, 560000, 33000, 1, 0, 2, 387281, 560000, 0, 4, 668000, 1000, 700000, 33000, 0, 4, 79000, 4000, 115281, 30000], // 1000 / 254
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 1046844, 30000, 1, 1, 2, 306281, 1048844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 0
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 901844, 4000, 1084844, 30000, 2, 993344, 17000, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 859844, 30000, 1, 1, 2, 306281, 861844, 4, 901844, 4000, 1084844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 1
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1206000, 1000, 1238000, 33000, 2, 1222000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1240000, 1000, 1272000, 33000, 2, 1256000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1172000, 1000, 1204000, 33000, 2, 1188000, 17000, 0, 0, 0, 4, 306281, 1000, 1130000, 33000, 1, 1, 2, 306281, 1132000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 2
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1070844, 1000, 1102844, 33000, 2, 1086844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1104844, 4000, 1220000, 30000, 2, 1162422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1036844, 1000, 1068844, 33000, 2, 1052844, 17000, 0, 2, 1257797, 17000, 0, 4, 306281, 4000, 994844, 30000, 1, 1, 2, 306281, 996844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 4
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 4, 858000, 5000, 928000, 29000, 2, 893000, 17000, 4, 928000, 9000, 944000, 25000, 2, 936000, 17000, 4, 954000, 5000, 1025000, 29000, 2, 989500, 17000, 4, 1025000, 9000, 1041000, 25000, 2, 1033000, 17000, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 1046844, 30000, 1, 1, 2, 306281, 856531, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 8
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 1046844, 30000, 1, 1, 2, 306281, 1048844, 0, 4, 1054844, 1000, 1086844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 16
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 1046844, 30000, 1, 1, 2, 484281, 1048844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 32
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 4, 443531, 7000, 444531, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 454031, 4000, 1046844, 30000, 1, 1, 2, 454031, 1048844, 0, 0, 0, 4, 298750, 4000, 335031, 30000], // 1280 / 64
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 984844, 1000, 1016844, 33000, 2, 1000844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1018844, 4000, 1134000, 30000, 2, 1076422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 950844, 1000, 982844, 33000, 2, 966844, 17000, 0, 0, 0, 4, 306281, 4000, 908844, 30000, 1, 1, 2, 306281, 910844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 128
    &[4, 36000, 1000, 68000, 33000, 2, 52000, 17000, 4, 79000, 4000, 115281, 30000, 2, 97141, 17000, 4, 116281, 1000, 148281, 33000, 2, 132281, 17000, 4, 150281, 1000, 182281, 33000, 2, 166281, 17000, 4, 184281, 1000, 216281, 33000, 2, 200281, 17000, 4, 1016000, 1000, 1048000, 33000, 2, 1032000, 17000, 4, 75500, 7000, 76500, 27000, 0, 0, 0, 0, 0, 4, 795000, 4000, 978000, 30000, 2, 886500, 17000, 4, 1050000, 1000, 1082000, 33000, 2, 1066000, 17000, 4, 562000, 5000, 632000, 29000, 2, 597000, 17000, 4, 632000, 9000, 648000, 25000, 2, 640000, 17000, 4, 658000, 5000, 729000, 29000, 2, 693500, 17000, 4, 729000, 9000, 745000, 25000, 2, 737000, 17000, 4, 982000, 1000, 1014000, 33000, 2, 998000, 17000, 0, 2, 1119797, 17000, 2, 234281, 34000, 4, 562000, 1000, 562000, 33000, 1, 0, 2, 387281, 562000, 4, 795000, 4000, 978000, 30000, 4, 761000, 1000, 793000, 33000, 0, 4, 79000, 4000, 115281, 30000], // 1280 / 255
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1070844, 1000, 1102844, 33000, 2, 1086844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1104844, 4000, 1220000, 30000, 2, 1162422, 17000, 4, 806000, 5000, 876000, 29000, 2, 841000, 17000, 4, 876000, 9000, 892000, 25000, 2, 884000, 17000, 4, 902000, 5000, 973000, 29000, 2, 937500, 17000, 4, 973000, 9000, 989000, 25000, 2, 981000, 17000, 4, 1036844, 1000, 1068844, 33000, 2, 1052844, 17000, 0, 2, 1257797, 17000, 0, 4, 306281, 4000, 994844, 30000, 1, 1, 2, 306281, 804531, 0, 4, 1002844, 1000, 1034844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 28
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1206000, 1000, 1238000, 33000, 2, 1222000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 985000, 4000, 1168000, 30000, 2, 1076500, 17000, 4, 1240000, 1000, 1272000, 33000, 2, 1256000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1172000, 1000, 1204000, 33000, 2, 1188000, 17000, 0, 0, 0, 4, 306281, 1000, 943000, 33000, 1, 1, 2, 306281, 945000, 4, 985000, 4000, 1168000, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 3
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1070844, 1000, 1102844, 33000, 2, 1086844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 849844, 4000, 1032844, 30000, 2, 941344, 17000, 4, 1104844, 4000, 1220000, 30000, 2, 1162422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1036844, 1000, 1068844, 33000, 2, 1052844, 17000, 0, 2, 1257797, 17000, 0, 4, 306281, 4000, 807844, 30000, 1, 1, 2, 306281, 809844, 4, 849844, 4000, 1032844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 5
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1154000, 1000, 1186000, 33000, 2, 1170000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1188000, 1000, 1220000, 33000, 2, 1204000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1120000, 1000, 1152000, 33000, 2, 1136000, 17000, 0, 2, 1257797, 17000, 0, 4, 306281, 1000, 1078000, 33000, 1, 1, 2, 306281, 1080000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 6
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 901844, 4000, 1084844, 30000, 2, 993344, 17000, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 4, 671000, 5000, 741000, 29000, 2, 706000, 17000, 4, 741000, 9000, 757000, 25000, 2, 749000, 17000, 4, 767000, 5000, 838000, 29000, 2, 802500, 17000, 4, 838000, 9000, 854000, 25000, 2, 846000, 17000, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 859844, 30000, 1, 1, 2, 306281, 669531, 4, 901844, 4000, 1084844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 9
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 901844, 4000, 1084844, 30000, 2, 993344, 17000, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 859844, 30000, 1, 1, 2, 306281, 861844, 4, 901844, 4000, 1084844, 30000, 4, 867844, 1000, 899844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 17
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 4, 858000, 5000, 928000, 29000, 2, 893000, 17000, 4, 928000, 9000, 944000, 25000, 2, 936000, 17000, 4, 954000, 5000, 1025000, 29000, 2, 989500, 17000, 4, 1025000, 9000, 1041000, 25000, 2, 1033000, 17000, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 306281, 4000, 1046844, 30000, 1, 1, 2, 306281, 856531, 0, 4, 1054844, 1000, 1086844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 24
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 4, 901844, 4000, 1084844, 30000, 2, 993344, 17000, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 859844, 30000, 1, 1, 2, 484281, 861844, 4, 901844, 4000, 1084844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 33
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 4, 443531, 7000, 444531, 27000, 0, 4, 901844, 4000, 1084844, 30000, 2, 993344, 17000, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 0, 4, 454031, 4000, 859844, 30000, 1, 1, 2, 454031, 861844, 4, 901844, 4000, 1084844, 30000, 0, 0, 4, 298750, 4000, 335031, 30000], // 1280 / 65
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1122844, 1000, 1154844, 33000, 2, 1138844, 17000, 4, 295250, 7000, 296250, 27000, 0, 4, 444531, 7000, 445531, 27000, 0, 4, 621531, 7000, 622531, 27000, 0, 0, 0, 4, 1156844, 4000, 1272000, 30000, 2, 1214422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1088844, 1000, 1120844, 33000, 2, 1104844, 17000, 0, 0, 2, 462031, 34000, 4, 632031, 4000, 1046844, 30000, 1, 1, 2, 632031, 1048844, 0, 0, 0, 4, 298750, 4000, 335031, 30000], // 1280 / 96
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 984844, 1000, 1016844, 33000, 2, 1000844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 763844, 4000, 946844, 30000, 2, 855344, 17000, 4, 1018844, 4000, 1134000, 30000, 2, 1076422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 950844, 1000, 982844, 33000, 2, 966844, 17000, 0, 0, 0, 4, 306281, 4000, 721844, 30000, 1, 1, 2, 306281, 723844, 4, 763844, 4000, 946844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 129
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 984844, 1000, 1016844, 33000, 2, 1000844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 1018844, 4000, 1134000, 30000, 2, 1076422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 950844, 1000, 982844, 33000, 2, 966844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 908844, 30000, 1, 1, 2, 484281, 910844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 160
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 932844, 1000, 964844, 33000, 2, 948844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 0, 0, 0, 0, 4, 966844, 4000, 1082000, 30000, 2, 1024422, 17000, 4, 665844, 5000, 736000, 29000, 2, 700922, 17000, 4, 736000, 9000, 752000, 25000, 2, 744000, 17000, 4, 761844, 5000, 833000, 29000, 2, 797422, 17000, 4, 833000, 9000, 849000, 25000, 2, 841000, 17000, 4, 898844, 1000, 930844, 33000, 2, 914844, 17000, 0, 2, 1119797, 17000, 2, 306281, 34000, 4, 665844, 4000, 665844, 30000, 1, 0, 2, 459281, 665844, 0, 4, 864844, 1000, 896844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1280 / 188
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1016000, 1000, 1048000, 33000, 2, 1032000, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 0, 0, 0, 0, 4, 1050000, 1000, 1082000, 33000, 2, 1066000, 17000, 4, 749000, 5000, 819000, 29000, 2, 784000, 17000, 4, 819000, 9000, 835000, 25000, 2, 827000, 17000, 4, 845000, 5000, 916000, 29000, 2, 880500, 17000, 4, 916000, 9000, 932000, 25000, 2, 924000, 17000, 4, 982000, 1000, 1014000, 33000, 2, 998000, 17000, 0, 2, 1119797, 17000, 2, 454031, 34000, 4, 749000, 1000, 749000, 33000, 1, 0, 2, 607031, 749000, 0, 4, 948000, 1000, 980000, 33000, 0, 4, 298750, 4000, 335031, 30000], // 1280 / 254
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1686844, 30000, 1, 1, 2, 306281, 1688844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 0
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 1541844, 4000, 1724844, 30000, 2, 1633344, 17000, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1499844, 30000, 1, 1, 2, 306281, 1501844, 4, 1541844, 4000, 1724844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 1
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1846000, 1000, 1878000, 33000, 2, 1862000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1880000, 1000, 1912000, 33000, 2, 1896000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1812000, 1000, 1844000, 33000, 2, 1828000, 17000, 0, 0, 0, 4, 306281, 1000, 1770000, 33000, 1, 1, 2, 306281, 1772000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 2
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1710844, 1000, 1742844, 33000, 2, 1726844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1744844, 4000, 1860000, 30000, 2, 1802422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1676844, 1000, 1708844, 33000, 2, 1692844, 17000, 0, 2, 1897797, 17000, 0, 4, 306281, 4000, 1634844, 30000, 1, 1, 2, 306281, 1636844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 4
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 4, 1498000, 5000, 1568000, 29000, 2, 1533000, 17000, 4, 1568000, 9000, 1584000, 25000, 2, 1576000, 17000, 4, 1594000, 5000, 1665000, 29000, 2, 1629500, 17000, 4, 1665000, 9000, 1681000, 25000, 2, 1673000, 17000, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1686844, 30000, 1, 1, 2, 306281, 1496531, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 8
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1686844, 30000, 1, 1, 2, 306281, 1688844, 0, 4, 1694844, 1000, 1726844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 16
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 1686844, 30000, 1, 1, 2, 484281, 1688844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 32
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 4, 443531, 7000, 444531, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 454031, 4000, 1686844, 30000, 1, 1, 2, 454031, 1688844, 0, 0, 0, 4, 298750, 4000, 335031, 30000], // 1920 / 64
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1624844, 1000, 1656844, 33000, 2, 1640844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1658844, 4000, 1774000, 30000, 2, 1716422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1590844, 1000, 1622844, 33000, 2, 1606844, 17000, 0, 0, 0, 4, 306281, 4000, 1548844, 30000, 1, 1, 2, 306281, 1550844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 128
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1656000, 1000, 1688000, 33000, 2, 1672000, 17000, 4, 295250, 7000, 296250, 27000, 0, 4, 444531, 7000, 445531, 27000, 0, 4, 621531, 7000, 622531, 27000, 0, 4, 1435000, 4000, 1618000, 30000, 2, 1526500, 17000, 4, 1690000, 1000, 1722000, 33000, 2, 1706000, 17000, 4, 1204000, 5000, 1274000, 29000, 2, 1239000, 17000, 4, 1274000, 9000, 1290000, 25000, 2, 1282000, 17000, 4, 1300000, 5000, 1371000, 29000, 2, 1335500, 17000, 4, 1371000, 9000, 1387000, 25000, 2, 1379000, 17000, 4, 1622000, 1000, 1654000, 33000, 2, 1638000, 17000, 0, 2, 1759797, 17000, 2, 462031, 34000, 4, 632031, 1000, 1393000, 33000, 1, 1, 2, 632031, 1202688, 4, 1435000, 4000, 1618000, 30000, 4, 1401000, 1000, 1433000, 33000, 0, 4, 298750, 4000, 335031, 30000], // 1920 / 255
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1710844, 1000, 1742844, 33000, 2, 1726844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1744844, 4000, 1860000, 30000, 2, 1802422, 17000, 4, 1446000, 5000, 1516000, 29000, 2, 1481000, 17000, 4, 1516000, 9000, 1532000, 25000, 2, 1524000, 17000, 4, 1542000, 5000, 1613000, 29000, 2, 1577500, 17000, 4, 1613000, 9000, 1629000, 25000, 2, 1621000, 17000, 4, 1676844, 1000, 1708844, 33000, 2, 1692844, 17000, 0, 2, 1897797, 17000, 0, 4, 306281, 4000, 1634844, 30000, 1, 1, 2, 306281, 1444531, 0, 4, 1642844, 1000, 1674844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 28
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1846000, 1000, 1878000, 33000, 2, 1862000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 1625000, 4000, 1808000, 30000, 2, 1716500, 17000, 4, 1880000, 1000, 1912000, 33000, 2, 1896000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1812000, 1000, 1844000, 33000, 2, 1828000, 17000, 0, 0, 0, 4, 306281, 1000, 1583000, 33000, 1, 1, 2, 306281, 1585000, 4, 1625000, 4000, 1808000, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 3
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1710844, 1000, 1742844, 33000, 2, 1726844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 1489844, 4000, 1672844, 30000, 2, 1581344, 17000, 4, 1744844, 4000, 1860000, 30000, 2, 1802422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1676844, 1000, 1708844, 33000, 2, 1692844, 17000, 0, 2, 1897797, 17000, 0, 4, 306281, 4000, 1447844, 30000, 1, 1, 2, 306281, 1449844, 4, 1489844, 4000, 1672844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 5
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1794000, 1000, 1826000, 33000, 2, 1810000, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1828000, 1000, 1860000, 33000, 2, 1844000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1760000, 1000, 1792000, 33000, 2, 1776000, 17000, 0, 2, 1897797, 17000, 0, 4, 306281, 1000, 1718000, 33000, 1, 1, 2, 306281, 1720000, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 6
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 1541844, 4000, 1724844, 30000, 2, 1633344, 17000, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 4, 1311000, 5000, 1381000, 29000, 2, 1346000, 17000, 4, 1381000, 9000, 1397000, 25000, 2, 1389000, 17000, 4, 1407000, 5000, 1478000, 29000, 2, 1442500, 17000, 4, 1478000, 9000, 1494000, 25000, 2, 1486000, 17000, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1499844, 30000, 1, 1, 2, 306281, 1309531, 4, 1541844, 4000, 1724844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 9
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 1541844, 4000, 1724844, 30000, 2, 1633344, 17000, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1499844, 30000, 1, 1, 2, 306281, 1501844, 4, 1541844, 4000, 1724844, 30000, 4, 1507844, 1000, 1539844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 17
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 4, 1498000, 5000, 1568000, 29000, 2, 1533000, 17000, 4, 1568000, 9000, 1584000, 25000, 2, 1576000, 17000, 4, 1594000, 5000, 1665000, 29000, 2, 1629500, 17000, 4, 1665000, 9000, 1681000, 25000, 2, 1673000, 17000, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 306281, 4000, 1686844, 30000, 1, 1, 2, 306281, 1496531, 0, 4, 1694844, 1000, 1726844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 24
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 4, 1541844, 4000, 1724844, 30000, 2, 1633344, 17000, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 1499844, 30000, 1, 1, 2, 484281, 1501844, 4, 1541844, 4000, 1724844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 33
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 295250, 7000, 296250, 27000, 0, 0, 0, 4, 443531, 7000, 444531, 27000, 0, 4, 1541844, 4000, 1724844, 30000, 2, 1633344, 17000, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 0, 4, 454031, 4000, 1499844, 30000, 1, 1, 2, 454031, 1501844, 4, 1541844, 4000, 1724844, 30000, 0, 0, 4, 298750, 4000, 335031, 30000], // 1920 / 65
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1762844, 1000, 1794844, 33000, 2, 1778844, 17000, 4, 295250, 7000, 296250, 27000, 0, 4, 444531, 7000, 445531, 27000, 0, 4, 621531, 7000, 622531, 27000, 0, 0, 0, 4, 1796844, 4000, 1912000, 30000, 2, 1854422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1728844, 1000, 1760844, 33000, 2, 1744844, 17000, 0, 0, 2, 462031, 34000, 4, 632031, 4000, 1686844, 30000, 1, 1, 2, 632031, 1688844, 0, 0, 0, 4, 298750, 4000, 335031, 30000], // 1920 / 96
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1624844, 1000, 1656844, 33000, 2, 1640844, 17000, 4, 147500, 7000, 148500, 27000, 0, 0, 0, 4, 295781, 7000, 296781, 27000, 0, 4, 1403844, 4000, 1586844, 30000, 2, 1495344, 17000, 4, 1658844, 4000, 1774000, 30000, 2, 1716422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1590844, 1000, 1622844, 33000, 2, 1606844, 17000, 0, 0, 0, 4, 306281, 4000, 1361844, 30000, 1, 1, 2, 306281, 1363844, 4, 1403844, 4000, 1586844, 30000, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 129
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1624844, 1000, 1656844, 33000, 2, 1640844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 1658844, 4000, 1774000, 30000, 2, 1716422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1590844, 1000, 1622844, 33000, 2, 1606844, 17000, 0, 0, 2, 314281, 34000, 4, 484281, 4000, 1548844, 30000, 1, 1, 2, 484281, 1550844, 0, 0, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 160
    &[4, 108000, 1000, 140000, 33000, 2, 124000, 17000, 4, 151000, 4000, 187281, 30000, 2, 169141, 17000, 4, 188281, 1000, 220281, 33000, 2, 204281, 17000, 4, 222281, 1000, 254281, 33000, 2, 238281, 17000, 4, 256281, 1000, 288281, 33000, 2, 272281, 17000, 4, 1572844, 1000, 1604844, 33000, 2, 1588844, 17000, 4, 147500, 7000, 148500, 27000, 0, 4, 296781, 7000, 297781, 27000, 0, 4, 473781, 7000, 474781, 27000, 0, 0, 0, 4, 1606844, 4000, 1722000, 30000, 2, 1664422, 17000, 4, 1308000, 5000, 1378000, 29000, 2, 1343000, 17000, 4, 1378000, 9000, 1394000, 25000, 2, 1386000, 17000, 4, 1404000, 5000, 1475000, 29000, 2, 1439500, 17000, 4, 1475000, 9000, 1491000, 25000, 2, 1483000, 17000, 4, 1538844, 1000, 1570844, 33000, 2, 1554844, 17000, 0, 2, 1759797, 17000, 2, 314281, 34000, 4, 484281, 4000, 1496844, 30000, 1, 1, 2, 484281, 1306531, 0, 4, 1504844, 1000, 1536844, 33000, 0, 4, 151000, 4000, 187281, 30000], // 1920 / 188
    &[4, 255750, 1000, 287750, 33000, 2, 271750, 17000, 4, 298750, 4000, 335031, 30000, 2, 316891, 17000, 4, 336031, 1000, 368031, 33000, 2, 352031, 17000, 4, 370031, 1000, 402031, 33000, 2, 386031, 17000, 4, 404031, 1000, 436031, 33000, 2, 420031, 17000, 4, 1656000, 1000, 1688000, 33000, 2, 1672000, 17000, 4, 295250, 7000, 296250, 27000, 0, 4, 444531, 7000, 445531, 27000, 0, 4, 621531, 7000, 622531, 27000, 0, 0, 0, 4, 1690000, 1000, 1722000, 33000, 2, 1706000, 17000, 4, 1391000, 5000, 1461000, 29000, 2, 1426000, 17000, 4, 1461000, 9000, 1477000, 25000, 2, 1469000, 17000, 4, 1487000, 5000, 1558000, 29000, 2, 1522500, 17000, 4, 1558000, 9000, 1574000, 25000, 2, 1566000, 17000, 4, 1622000, 1000, 1654000, 33000, 2, 1638000, 17000, 0, 2, 1759797, 17000, 2, 462031, 34000, 4, 632031, 1000, 1580000, 33000, 1, 1, 2, 632031, 1389688, 0, 4, 1588000, 1000, 1620000, 33000, 0, 4, 298750, 4000, 335031, 30000], // 1920 / 254
];
