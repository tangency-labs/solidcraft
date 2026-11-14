//! The application bar from plain data, no app: what each click returns.
use super::*;

const HANDLES: Handles<'static> = Handles {
    home: &["t:home"],
    home_rect: &["t:home"],
    file: &["t:file"],
    file_rect: &["t:file"],
    plugins: "t:plugins",
    button: &["t:"],
    separator: "t:separator:",
    search: "t:search",
    report: "t:report",
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
    /// The bar has a Plugins menu.
    plugins: bool,
    time: std::cell::Cell<f64>,
    out: std::cell::RefCell<Option<BarOut>>,
}

impl BarRig {
    fn new(width: f32) -> BarRig {
        BarRig::named(width, "App")
    }

    /// The bar with the app's name `name`.
    fn named(width: f32, name: &'static str) -> BarRig {
        let rig =
            BarRig { ctx: crate::test_support::context(), width, name, plugins: false, time: std::cell::Cell::new(0.0), out: Default::default() };
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
            mark: &|_, _| {},
            name: self.name,
            home: BarButton { id: "home", icon: "home", tip: "Home", enabled: true, narrow: false, wide_only: false },
            file: "File",
            file_open: false,
            plugins: self.plugins.then_some(("Plugins", false, "Plugins menu")),
            actions: vec![button("save", true, false), button("undo", false, true)],
            quick: 2,
            help: BarButton { id: "help", icon: "help", tip: "Help", enabled: true, narrow: false, wide_only: false },
            settings: BarButton { id: "settings", icon: "settings", tip: "Settings", enabled: true, narrow: false, wide_only: false },
            report: Report { label: "Report", tip: "Report a problem", icon_only: false },
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

/// The Plugins menu's button follows File without overlapping it or Save, and returns its own
/// action; without one the bar is as before.
#[test]
fn the_plugins_button_sits_between_file_and_the_file_actions() {
    for width in [1200.0, 600.0] {
        let rig = BarRig::new(width);
        let save_without = solidcraft_ui_registry::rect_of("t:save").expect("Save is drawn");
        assert!(solidcraft_ui_registry::rect_of("t:plugins").is_none());
        let rig = BarRig { plugins: true, ..rig };
        rig.frame(Vec::new());
        let file = solidcraft_ui_registry::rect_of("t:file").expect("File is drawn");
        let plugins = solidcraft_ui_registry::rect_of("t:plugins").expect("Plugins is drawn");
        let save = solidcraft_ui_registry::rect_of("t:save").expect("Save is drawn");
        assert!(plugins.left() > file.right() && plugins.right() < save.left(), "{file:?} {plugins:?} {save:?}");
        assert!(save.left() > save_without.left(), "the file actions move right by its width");
        assert_eq!(rig.click("t:plugins"), vec![BarAction::Plugins { rect: plugins }]);
    }
}

/// Between the compact width and the room the Plugins label needs, the bar leaves Plugins out (the
/// app lists it in the File menu) and keeps exactly what it showed before there was a Plugins menu.
#[test]
fn between_compact_and_the_labels_room_plugins_moves_to_the_file_menu() {
    for width in [645.0, 660.0] {
        let rig = BarRig::new(width);
        let before = rig.out.borrow().as_ref().map(|out| (out.quick, out.tabs_start)).unwrap();
        let save = solidcraft_ui_registry::rect_of("t:save");
        let rig = BarRig { plugins: true, ..rig };
        rig.frame(Vec::new());
        assert!(solidcraft_ui_registry::rect_of("t:plugins").is_none(), "{width}: no Plugins button");
        assert!(solidcraft_ui_registry::rect_of("t:undo").is_some(), "{width}: Undo stays");
        assert_eq!(solidcraft_ui_registry::rect_of("t:save"), save, "{width}: Save stays put");
        let out = rig.out.borrow();
        let out = out.as_ref().unwrap();
        assert!(out.plugins_in_file, "{width}: the app is told to list Plugins in File");
        assert_eq!((out.quick, out.tabs_start), before, "{width}: the quick-access room and tabs stay put");
    }
    for (width, label) in [(1200.0, true), (600.0, false)] {
        let rig = BarRig { plugins: true, ..BarRig::new(width) };
        rig.frame(Vec::new());
        let plugins = solidcraft_ui_registry::rect_of("t:plugins").expect("Plugins is drawn");
        assert_eq!(plugins.width() > 40.0, label, "{width}: label when wide, icon when compact");
        assert!(!rig.out.borrow().as_ref().unwrap().plugins_in_file);
    }
}

/// A compact bar leaves out the wide-only buttons and the quick-access room.
#[test]
fn a_compact_bar_keeps_only_what_fits() {
    let rig = BarRig::new(600.0);
    assert!(solidcraft_ui_registry::rect_of("t:save").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:undo").is_none(), "Undo is left out");
    assert!(rig.out.borrow().as_ref().is_some_and(|out| out.quick.is_none()));
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

const VIEW_MENU: [TabMenuRow<'static>; 3] = [
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
        tip: "Model",
    },
    ViewTab {
        name: "Sketch 2",
        icon: "sketch",
        accent: egui::Color32::GREEN,
        underline: egui::Color32::GREEN,
        state: ViewState::SourceChanged,
        active: false,
        tip: "Sketch 2",
    },
    ViewTab {
        name: "Sheet 1",
        icon: "sheet",
        accent: egui::Color32::RED,
        underline: egui::Color32::RED,
        state: ViewState::Deleted,
        active: false,
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
        let first = DocTab { views: if self.views.is_some() { &VIEWS } else { &[] }, ..tab("One") };
        let (mut text, mut focused) = (self.text.borrow().clone(), self.focused.get());
        let mut strip = TabStrip {
            tabs: vec![first, tab("Two"), tab("Three")],
            selected: 0,
            shown_selected: Some(0),
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
            switcher_menu: &SWITCHER_MENU,
            tear_off_below: 40.0,
        };
        let (x0, x1) = if self.views.is_some() { (20.0, 780.0) } else { (100.0, 700.0) };
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
/// the active view and the "N more" button; otherwise a switcher. One view or none is a plain tab.
#[test]
fn the_selected_tab_falls_back_to_a_switcher_where_the_pill_has_no_room() {
    assert_eq!(selected_layout(0, true, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(1, true, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(3, true, 300.0, 300.0), SelectedLayout::Pill);
    assert_eq!(selected_layout(3, true, 300.0, 299.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(3, false, 300.0, 900.0), SelectedLayout::Switcher);
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
    let from = rig.at("t:view:0");
    let to = solidcraft_ui_registry::rect_of("t:view:2").map(|r| r.right_center() - vec2(4.0, 0.0)).expect("view 2 is drawn");
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    actions.extend(rig.frame(vec![press(from, true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![press(to, false)]));
    assert_eq!(actions, vec![TabAction::ReorderView { from: 0, to: 2 }]);
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
    for glyph in ["model", "sketch", "sheet", "render", "animation", "rename", "window", "close", "duplicate", "reopen"] {
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
