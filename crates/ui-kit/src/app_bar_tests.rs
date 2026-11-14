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

const TAB_HANDLES: TabHandles<'static> =
    TabHandles { tab: "t:tab:", close: "t:close:", more: "t:more", new: "t:new", rename: "t:rename", menu_row: "t:menu:", more_row: "t:list:" };

const TAB_MENU: [TabMenuRow<'static>; 2] = [
    TabMenuRow { id: "close", label: "Close", key_on_selected: "", needs_others: false },
    TabMenuRow { id: "others", label: "Close Others", key_on_selected: "", needs_others: true },
];

/// Three document tabs, the first selected, in a strip 600 wide.
struct TabsRig {
    ctx: egui::Context,
    time: std::cell::Cell<f64>,
}

impl TabsRig {
    fn new() -> TabsRig {
        let rig = TabsRig { ctx: crate::test_support::context(), time: std::cell::Cell::new(0.0) };
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
        let tab = |name: &'static str| DocTab { name, list_name: name, dirty: false, place: "Not saved yet", security: None };
        let mut strip = TabStrip {
            tabs: vec![tab("One"), tab("Two"), tab("Three")],
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
        };
        let mut actions = Vec::new();
        crate::test_support::raw_frame(&self.ctx, input, |ui| {
            actions = tabs(ui, &mut strip, Rect::from_min_size(Pos2::ZERO, vec2(800.0, HEIGHT)), 100.0, 700.0);
        });
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
