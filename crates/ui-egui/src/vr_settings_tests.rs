//! VR settings (`vr_settings`, docs/vr.md): the defaults are the documented buttons and ways of
//! working, a settings file can hold anything and is made sane, older layouts migrate once, and
//! saved settings keep loading.

use crate::vr_settings::*;
use serde_json::json;

/// A field of [`VrSettings`] that holds a number, to change it in a table of tests.
type Field = fn(&mut VrSettings) -> &mut f32;

/// Every number of the settings with its name and the range `sane` keeps it in.
fn numbers() -> [(&'static str, Field, f32, f32); 24] {
    [
        ("hold_time", |s| &mut s.hold_time, 0.1, 2.0),
        ("menu_size", |s| &mut s.menu_size, 0.04, 0.3),
        ("menu_bubble", |s| &mut s.menu_bubble, 0.015, 0.15),
        ("stick_label_gap", |s| &mut s.stick_label_gap, 0.0, 0.15),
        ("stick_label_lift", |s| &mut s.stick_label_lift, -0.05, 0.1),
        ("menu_grow", |s| &mut s.menu_grow, 1.0, 2.0),
        ("section_wait", |s| &mut s.section_wait, 0.0, 2.0),
        ("sign_size", |s| &mut s.sign_size, 0.3, 3.0),
        ("fade", |s| &mut s.fade, 0.05, 1.0),
        ("belt_curve", |s| &mut s.belt_curve, 0.2, 5.0),
        ("grid", |s| &mut s.grid, 0.0, 1.0),
        ("horizon", |s| &mut s.horizon, 0.0, 1.0),
        ("grid_reach", |s| &mut s.grid_reach, 1.0, 100.0),
        ("browser_height", |s| &mut s.browser_height, 0.1, 1.0),
        ("browser_width", |s| &mut s.browser_width, 200.0, 600.0),
        ("ray_length", |s| &mut s.ray_length, 0.3, 20.0),
        ("haptics", |s| &mut s.haptics, 0.0, 2.0),
        ("first_distance", |s| &mut s.first_distance, 0.2, 5.0),
        ("first_size", |s| &mut s.first_size, 0.05, 5.0),
        ("first_drop", |s| &mut s.first_drop, -1.0, 1.5),
        ("push_speed", |s| &mut s.push_speed, 0.05, 5.0),
        ("belt_drop", |s| &mut s.belt_drop, 0.2, 1.2),
        ("belt_distance", |s| &mut s.belt_distance, 0.1, 1.0),
        ("turn_speed", |s| &mut s.turn_speed, 5.0, 360.0),
    ]
}

/// `left` on the left hand, `right` on the right.
fn binding(left: &[Input], right: &[Input]) -> Binding {
    Binding { left: left.to_vec(), right: right.to_vec() }
}

/// Settings unlike the defaults in every kind of field, at the current layout.
fn customised() -> VrSettings {
    VrSettings {
        select: binding(&[Input::Primary], &[Input::Trigger, Input::StickClick]),
        grab: binding(&[Input::Trigger], &[]),
        menu: Binding::default(),
        undo: binding(&[], &[Input::Primary]),
        stick: StickAction::UndoRedo,
        hold_stick: false,
        hold_sticks: HoldSticks::HoldingMoves,
        two_hands: TwoHands::Lead,
        scale_readout: ScaleReadout::Never,
        browser_side: BrowserSide::Above,
        ring: vec![RingAction::Undo, RingAction::Group(3)],
        grid: 0.25,
        belt_drop: 0.9,
        belt_distance: 0.8,
        ray_length: 7.5,
        ..VrSettings::default()
    }
}

/// The buttons out of the box are the table in docs/vr.md: the trigger selects and the grip
/// grabs on both hands, the right B (or either Menu button) opens the ring menu, the left X and Y
/// undo and redo, and the stick is left to what the hands do.
#[test]
fn the_default_buttons_are_the_ones_docs_vr_lists() {
    let d = VrSettings::default();
    assert_eq!(d.select, binding(&[Input::Trigger], &[Input::Trigger]));
    assert_eq!(d.grab, binding(&[Input::Grip], &[Input::Grip]));
    assert_eq!(d.menu, binding(&[Input::Menu], &[Input::Secondary, Input::Menu]));
    assert_eq!(d.undo, binding(&[Input::Primary], &[]));
    assert_eq!(d.redo, binding(&[Input::Secondary], &[]));
    assert!(d.put_back.is_empty() && d.section.is_empty(), "Put back and Section have no button of their own");
    assert_eq!(d.stick, StickAction::Nothing);
    assert_eq!(d.layout, LAYOUT, "fresh settings are for the current layout");
}

/// Holding, both hands, the belt and the look out of the box are as docs/vr.md describes them:
/// both hands equal, the right stick moves, the scale shown while scaling, the Browser left of
/// the belt, joints drawn, no passthrough, and the model 0.6 m off and 0.4 m across.
#[test]
fn the_default_ways_of_working_are_the_documented_ones() {
    let d = VrSettings::default();
    assert_eq!(d.two_hands, TwoHands::Equal);
    assert_eq!(d.hold_sticks, HoldSticks::RightMoves);
    assert!(d.hold_stick && d.stick_labels && d.snap_real_size);
    assert_eq!(d.scale_readout, ScaleReadout::WhileScaling);
    assert_eq!(d.browser_side, BrowserSide::Left);
    assert!(d.belt && d.joints && !d.passthrough);
    assert_eq!((d.first_distance, d.first_size), (0.6, 0.4));
}

/// The ring menu out of the box holds the documented slots (Show/Hide, Isolate, Show all,
/// Section, View ▸, Edit ▸), View holds Put back, Explode and Real room, Edit holds Undo and Redo,
/// and the groups' icons are ones the page offers.
#[test]
fn the_default_ring_is_the_documented_one() {
    let d = VrSettings::default();
    assert_eq!(
        d.ring,
        vec![RingAction::ToggleHide, RingAction::Isolate, RingAction::ShowAll, RingAction::Section, RingAction::Group(0), RingAction::Group(1)]
    );
    assert_eq!(d.ring_groups.len(), GROUPS);
    assert_eq!(d.ring_groups[0].items, vec![RingAction::PutBack, RingAction::Explode, RingAction::Passthrough]);
    assert_eq!(d.ring_groups[1].items, vec![RingAction::Undo, RingAction::Redo]);
    for g in d.ring_groups.iter().filter(|g| !g.name.is_empty()) {
        assert!(GROUP_ICONS.contains(&g.icon.as_str()), "{} has icon {}", g.name, g.icon);
    }
    assert_eq!(RingAction::all().iter().filter(|a| matches!(a, RingAction::Group(_))).count(), GROUPS);
}

/// The defaults are already sane: `sane` changes nothing in them, so every default number lies
/// in its range.
#[test]
fn the_defaults_are_left_as_they_are_by_sane() {
    assert_eq!(VrSettings::default().sane(), VrSettings::default());
}

/// A number that is not one (NaN, either infinity) from a settings file is the default again,
/// for every number of the settings.
#[test]
fn sane_puts_numbers_that_are_not_numbers_back_to_their_defaults() {
    let d = VrSettings::default();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for (name, field, _, _) in numbers() {
            let mut s = VrSettings::default();
            *field(&mut s) = bad;
            let mut want = d.clone();
            assert_eq!(*field(&mut s.sane()), *field(&mut want), "{name} = {bad}");
        }
    }
}

/// Huge, negative and tiny numbers are kept to each number's range: below it is its start, above
/// it its end.
#[test]
fn sane_clamps_out_of_range_numbers_into_their_ranges() {
    for (name, field, lo, hi) in numbers() {
        for (bad, want) in [(1e30, hi), (f32::MAX, hi), (-1e30, lo), (f32::MIN, lo), (lo - 1.0, lo), (hi + 1.0, hi)] {
            let mut s = VrSettings::default();
            *field(&mut s) = bad;
            assert_eq!(*field(&mut s.sane()), want, "{name} = {bad}");
        }
        let mut s = VrSettings::default();
        *field(&mut s) = hi;
        assert_eq!(*field(&mut s.sane()), hi, "{name}'s own end is kept");
    }
}

/// The Browser's width is a whole number of points.
#[test]
fn sane_rounds_the_browsers_width_to_whole_points() {
    let s = VrSettings { browser_width: 333.6, ..VrSettings::default() };
    assert_eq!(s.sane().browser_width, 334.0);
}

/// A hand-edited file that repeats an input many times on a hand keeps each input once, in the
/// order it first came, on each hand of each action.
#[test]
fn sane_keeps_each_input_once_on_each_hand() {
    let mut many = Vec::new();
    for _ in 0..10_000 {
        many.extend([Input::Grip, Input::Trigger, Input::Grip]);
    }
    let s = VrSettings { select: binding(&many, &[Input::Menu, Input::Menu]), section: binding(&[], &many), ..VrSettings::default() }.sane();
    assert_eq!(s.select, binding(&[Input::Grip, Input::Trigger], &[Input::Menu]));
    assert_eq!(s.section, binding(&[], &[Input::Grip, Input::Trigger]));
}

/// The first ring keeps at most eight slots, without empty slots or groups that do not exist
/// (a group number from a file can be anything up to 255).
#[test]
fn sane_caps_the_first_ring_at_eight_real_slots() {
    let mut ring = vec![RingAction::Empty, RingAction::Group(4), RingAction::Group(255)];
    for _ in 0..50 {
        ring.extend([RingAction::Undo, RingAction::Empty, RingAction::Group(7)]);
    }
    let s = VrSettings { ring, ..VrSettings::default() }.sane();
    assert_eq!(s.ring, vec![RingAction::Undo; RING_SLOTS]);
    let s = VrSettings { ring: vec![RingAction::Group(3), RingAction::Empty, RingAction::Group(0)], ..VrSettings::default() }.sane();
    assert_eq!(s.ring, vec![RingAction::Group(3), RingAction::Group(0)], "the groups there are stay");
}

/// The groups are always exactly four: more from a file are dropped, missing ones are empty
/// groups. Each holds at most eight actions, and no groups inside a group or empty slots; its
/// name and icon stay as they are.
#[test]
fn sane_keeps_four_groups_of_at_most_eight_actions() {
    let mut items = vec![RingAction::Group(0), RingAction::Empty];
    for _ in 0..20 {
        items.extend([RingAction::Hide, RingAction::Group(1)]);
    }
    let group = RingGroup { name: "Mine".into(), icon: "joint".into(), items };
    let s = VrSettings { ring_groups: vec![group; 10], ..VrSettings::default() }.sane();
    assert_eq!(s.ring_groups.len(), GROUPS);
    for g in &s.ring_groups {
        assert_eq!(g.items, vec![RingAction::Hide; RING_SLOTS]);
        assert_eq!((g.name.as_str(), g.icon.as_str()), ("Mine", "joint"));
    }
    let s = VrSettings { ring_groups: Vec::new(), ..VrSettings::default() }.sane();
    assert_eq!(s.ring_groups, vec![RingGroup::default(); GROUPS], "a file without groups gets four empty ones");
}

/// What `sane` has no range for (choices, switches, the layout) is kept as it is.
#[test]
fn sane_leaves_choices_and_switches_alone() {
    let s = customised();
    assert_eq!(s.sane(), s);
}

/// Settings of the current layout are not migrated: whatever the user chose stays.
#[test]
fn migrate_leaves_the_current_layout_alone() {
    let mut s = customised();
    s.migrate();
    assert_eq!(s, customised());
}

/// Settings saved by a newer SolidCraft (a layout after this one) are left as they are, layout
/// number included, so going back to the newer version finds them unchanged.
#[test]
fn migrate_leaves_a_layout_from_a_newer_version_alone() {
    for layout in [LAYOUT + 1, 99, u32::MAX] {
        let mut s = VrSettings { layout, ..customised() };
        s.migrate();
        assert_eq!(s, VrSettings { layout, ..customised() }, "layout {layout}");
    }
}

/// Layout 3 settings get only layout 4's change: both hands equal. The belt and the buttons the
/// user set stay.
#[test]
fn layout_3_gets_only_the_new_two_hands_default() {
    let mut s = VrSettings { layout: 3, ..customised() };
    s.migrate();
    assert_eq!(s, VrSettings { two_hands: TwoHands::Equal, ..customised() });
}

/// Layout 2 settings also get layout 3's belt place (higher and further out); the buttons stay.
#[test]
fn layout_2_also_gets_the_belts_new_place() {
    let d = VrSettings::default();
    let mut s = VrSettings { layout: 2, ..customised() };
    s.migrate();
    assert_eq!(s, VrSettings { two_hands: d.two_hands, belt_drop: d.belt_drop, belt_distance: d.belt_distance, ..customised() });
}

/// Layouts 0 (settings saved before there was a layout) and 1 get the buttons and the stick of
/// this layout, everything else they hold is kept, and only once: a button changed afterwards
/// stays.
#[test]
fn layouts_0_and_1_get_the_buttons_and_stick_once() {
    let d = VrSettings::default();
    for layout in [0, 1] {
        let mut s = VrSettings { layout, ..customised() };
        s.migrate();
        let want = VrSettings {
            select: d.select.clone(),
            grab: d.grab.clone(),
            menu: d.menu.clone(),
            put_back: d.put_back.clone(),
            section: d.section.clone(),
            undo: d.undo.clone(),
            redo: d.redo.clone(),
            stick: d.stick,
            hold_stick: d.hold_stick,
            two_hands: d.two_hands,
            belt_drop: d.belt_drop,
            belt_distance: d.belt_distance,
            ..customised()
        };
        assert_eq!(s, want, "layout {layout}");
        s.select = binding(&[Input::Menu], &[]);
        s.migrate();
        assert_eq!(s.select, binding(&[Input::Menu], &[]), "layout {layout}: migrated once");
    }
}

/// The settings saved with the preferences (JSON) read back the same, ring groups included.
#[test]
fn the_settings_read_back_as_the_preferences_saved_them() {
    let p = crate::prefs::Prefs { vr: customised(), ..crate::prefs::Prefs::default() };
    let saved = serde_json::to_value(&p).unwrap();
    assert_eq!(saved["vr"]["ring"], json!(["undo", {"group": 3}]), "a group slot is saved with its number");
    let read = crate::prefs::lenient(&saved);
    assert_eq!(read.vr, customised());
    let text = serde_json::to_string_pretty(&p).unwrap();
    assert_eq!(serde_json::from_str::<VrSettings>(&serde_json::to_string(&p.vr).unwrap()).unwrap(), customised());
    assert!(text.contains("\"two_hands\": \"lead\""), "choices are saved by name");
}

/// Saved VR settings missing fields (saved before those settings existed) read with the missing
/// ones at their defaults; a file without a layout number is layout 0, which reading it brings
/// up to this one (`migrate`).
#[test]
fn saved_settings_missing_fields_read_with_their_defaults() {
    let read = crate::prefs::lenient(&json!({"vr": {"layout": LAYOUT, "two_hands": "lead", "grid": 0.5}}));
    assert_eq!(read.vr, VrSettings { two_hands: TwoHands::Lead, grid: 0.5, ..VrSettings::default() });
    let one_hand = crate::prefs::lenient(&json!({"vr": {"layout": LAYOUT, "select": {"left": ["grip"]}}})).vr;
    assert_eq!(one_hand.select, binding(&[Input::Grip], &[]), "a hand missing from a binding has no inputs");
    let old = crate::prefs::lenient(&json!({"vr": {"select": {"left": ["grip"]}, "grid": 0.5}})).vr;
    assert_eq!(old, VrSettings { grid: 0.5, ..VrSettings::default() }, "no layout: saved before there was one, migrated as read");
    assert_eq!(crate::prefs::lenient(&json!({})).vr, VrSettings::default(), "no VR settings saved: the defaults");
}

/// Settings of an older layout are migrated as they are read (from the preferences file or the
/// control channel), once, so the Preferences page and the headset only ever see the current
/// layout and neither changes the preferences while drawing; a newer layout is read as saved.
#[test]
fn an_older_layout_is_migrated_when_it_is_read() {
    let saved = |layout: u32| serde_json::to_value(VrSettings { layout, ..customised() }).unwrap();
    let d = VrSettings::default();
    let read = VrSettings::from_saved(saved(3)).unwrap();
    assert_eq!(read, VrSettings { two_hands: d.two_hands, ..customised() }, "layout 3: only layout 4's change");
    assert_eq!(read.layout, LAYOUT);
    let read = crate::prefs::lenient(&json!({"vr": saved(1)})).vr;
    assert_eq!((read.layout, read.select.clone(), read.two_hands), (LAYOUT, d.select.clone(), d.two_hands), "through the preferences too");
    assert_eq!(read.ring, customised().ring, "what migration does not touch stays");
    let newer = VrSettings::from_saved(saved(LAYOUT + 1)).unwrap();
    assert_eq!(newer, VrSettings { layout: LAYOUT + 1, ..customised() });
    // Read again, settings already migrated are not migrated again.
    let again = VrSettings::from_saved(serde_json::to_value(VrSettings { select: binding(&[Input::Menu], &[]), ..read.clone() }).unwrap()).unwrap();
    assert_eq!(again.select, binding(&[Input::Menu], &[]));
}

/// Huge lists and long names from a settings file or the control channel come back bounded as
/// they are read, so the Preferences page never draws a dropdown for each of a million slots:
/// at most eight ring slots, exactly four groups of at most eight actions, each input once a
/// hand, names and icons of at most [`NAME_CHARS`] characters.
#[test]
fn huge_lists_and_long_names_are_bounded_as_they_are_read() {
    let many_inputs: Vec<&str> = ["grip", "trigger", "menu"].iter().copied().cycle().take(30_000).collect();
    let many_slots: Vec<&str> = vec!["undo"; 10_000];
    let group = json!({"name": "n".repeat(100_000), "icon": "é".repeat(10_000), "items": many_slots});
    let saved = json!({
        "layout": LAYOUT,
        "select": {"left": many_inputs, "right": many_inputs},
        "undo": {"right": many_inputs},
        "ring": many_slots,
        "ring_groups": vec![group; 10],
    });
    for read in [VrSettings::from_saved(saved.clone()).unwrap(), crate::prefs::lenient(&json!({"vr": saved})).vr] {
        let three = binding(&[Input::Grip, Input::Trigger, Input::Menu], &[Input::Grip, Input::Trigger, Input::Menu]);
        assert_eq!(read.select, three);
        assert_eq!(read.undo, binding(&[], &[Input::Grip, Input::Trigger, Input::Menu]));
        assert_eq!(read.ring, vec![RingAction::Undo; RING_SLOTS]);
        assert_eq!(read.ring_groups.len(), GROUPS);
        for g in &read.ring_groups {
            assert_eq!(g.items, vec![RingAction::Undo; RING_SLOTS]);
            assert_eq!(g.name, "n".repeat(NAME_CHARS));
            assert_eq!(g.icon, "é".repeat(NAME_CHARS), "cut by characters, not bytes");
        }
        assert_eq!(read.sane(), read, "nothing left for `sane` to cut");
    }
    // Fewer groups than there are: the rest are empty ones.
    let read = VrSettings::from_saved(json!({"layout": LAYOUT, "ring_groups": [{"name": "Mine"}]})).unwrap();
    assert_eq!(read.ring_groups.len(), GROUPS);
    assert_eq!(read.ring_groups[0].name, "Mine");
}

/// Fields this version does not know (a newer SolidCraft's settings, a hand edit) are passed
/// over, in the settings, a binding and a group alike, and the rest still reads.
#[test]
fn unknown_fields_in_saved_settings_are_passed_over() {
    let read = crate::prefs::lenient(&json!({"vr": {
        "layout": LAYOUT,
        "teleport": true,
        "grid": 0.5,
        "grab": {"left": ["trigger"], "feet": ["stomp"]},
        "ring_groups": [{"name": "Mine", "colour": "red"}],
    }}));
    assert_eq!(read.vr.grid, 0.5);
    assert_eq!(read.vr.grab, binding(&[Input::Trigger], &[]));
    assert_eq!(read.vr.ring_groups[0].name, "Mine");
}

/// A VR value this version cannot read (a choice or action it does not know, a wrong type) puts
/// the VR settings back to their defaults and leaves every other preference as saved; hostile
/// VR settings that are no map of settings at all fall back to the defaults as a whole and leave
/// every other preference as saved; hostile numbers that do read are then made sane.
#[test]
fn unreadable_vr_settings_fall_back_without_losing_other_preferences() {
    for bad in [json!("garbage"), json!(null), json!(7), json!([1, 2])] {
        let read = crate::prefs::lenient(&json!({"theme": "light", "vr": bad}));
        assert_eq!(read.theme, "light", "{bad}");
        assert_eq!(read.vr, VrSettings::default(), "{bad}");
    }
    let read = crate::prefs::lenient(&json!({"vr": {"layout": LAYOUT, "grid": 1e300, "ray_length": -5, "turn_speed": 1e9}})).vr.sane();
    assert_eq!(read.grid, VrSettings::default().grid, "a number too big for the settings is no number: the default");
    assert_eq!((read.ray_length, read.turn_speed), (0.3, 360.0));
}

/// One VR setting this version cannot read (a newer version's choice, input, ring action, a
/// wrong type) takes its default alone: the other VR settings stay as saved (opening the
/// preferences of a newer version in an older one).
#[test]
fn one_unreadable_vr_setting_takes_its_default_and_keeps_the_others() {
    let d = VrSettings::default();
    let kept = 0.5_f32;
    assert_ne!(d.grid, kept);
    for bad in [
        json!({"two_hands": "pinch"}),
        json!({"ring": ["teleport"]}),
        json!({"ring": [{"group": 300}]}),
        json!({"select": {"left": ["foot_pedal"]}}),
        json!({"horizon": "lots"}),
    ] {
        let mut saved = bad.as_object().cloned().unwrap_or_default();
        saved.insert("layout".into(), json!(LAYOUT));
        saved.insert("grid".into(), json!(kept));
        let read = crate::prefs::lenient(&json!({"theme": "light", "vr": saved}));
        assert_eq!(read.theme, "light", "{bad}");
        assert_eq!(read.vr.grid, kept, "{bad}: the readable setting stays");
        assert_eq!(read.vr.layout, LAYOUT, "{bad}");
        let key = bad.as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
        let fallback = serde_json::to_value(&d).ok().and_then(|v| v.get(&key).cloned());
        let got = serde_json::to_value(&read.vr).ok().and_then(|v| v.get(&key).cloned());
        assert_eq!(got, fallback, "{bad}: the unreadable one takes its default");
    }
    // A layout that is no number is left out: read as an old layout, which is migrated as read.
    let read = VrSettings::from_saved(json!({"layout": -1, "grid": kept})).unwrap_or_default();
    assert_eq!((read.layout, read.grid), (LAYOUT, kept));
}

/// A saved map with a great many keys is read without looking at all of them (no layout among
/// them: migrated to the defaults of this one).
#[test]
fn a_saved_map_of_very_many_keys_is_read_with_the_defaults() {
    let mut saved = serde_json::Map::new();
    for i in 0..100_000 {
        saved.insert(format!("unknown_{i}"), json!(i));
    }
    let read = VrSettings::from_saved(serde_json::Value::Object(saved));
    assert_eq!(read.ok(), Some(VrSettings::default()));
}

/// A chip's Remove takes that input off that hand only; the other hand and the other inputs
/// stay, and removing what is not there changes nothing.
#[test]
fn a_removed_chip_takes_only_that_input_off_that_hand() {
    let mut b = binding(&[Input::Trigger, Input::Grip], &[Input::Trigger]);
    b.remove(0, Input::Trigger);
    assert_eq!(b, binding(&[Input::Grip], &[Input::Trigger]));
    b.remove(0, Input::Menu);
    b.remove(1, Input::Grip);
    assert_eq!(b, binding(&[Input::Grip], &[Input::Trigger]));
    b.remove(1, Input::Trigger);
    b.remove(0, Input::Grip);
    assert!(b.is_empty());
}

/// The "+" menu offers a hand only the inputs it does not have, an input added is put at the
/// end once, and adding it again does nothing.
#[test]
fn an_added_input_is_there_once_and_no_longer_offered() {
    let mut b = binding(&[Input::Trigger], &[]);
    assert_eq!(b.unused(0), Input::ALL[1..].to_vec());
    assert_eq!(b.unused(1), Input::ALL.to_vec());
    b.add(1, Input::Menu);
    b.add(1, Input::Menu);
    b.add(0, Input::Trigger);
    assert_eq!(b, binding(&[Input::Trigger], &[Input::Menu]));
    assert!(!b.unused(1).contains(&Input::Menu));
    for input in Input::ALL {
        b.add(0, input);
    }
    assert!(b.unused(0).is_empty(), "every input taken: nothing more to offer");
    assert!(b.reads(0, Input::StickClick) && !b.reads(1, Input::StickClick));
}

/// A hand past the right one (there are two) has no inputs: it reads nothing, is offered
/// nothing, and adding or removing on it changes nothing.
#[test]
fn a_hand_past_the_right_one_changes_nothing() {
    let mut b = binding(&[Input::Trigger], &[Input::Grip]);
    for hand in [2, 7, usize::MAX] {
        b.add(hand, Input::Menu);
        b.remove(hand, Input::Trigger);
        assert!(b.on(hand).is_empty() && b.unused(hand).is_empty() && !b.reads(hand, Input::Trigger), "hand {hand}");
    }
    assert_eq!(b, binding(&[Input::Trigger], &[Input::Grip]));
}

/// A group named by the user shows as typed; a built-in group shows in the user's language, not
/// as its catalogue key.
#[test]
fn a_groups_name_is_the_users_words_or_the_catalogues() {
    crate::i18n::install();
    let mine = RingGroup { name: "My tools".into(), ..RingGroup::default() };
    assert_eq!(mine.shown_name(), "My tools");
    let view = &VrSettings::default().ring_groups[0];
    assert!(!view.shown_name().is_empty() && view.shown_name() != view.name, "{}", view.shown_name());
}

/// A ring's slot choosers fill a settings row as far as they fit and go on in the next, each row
/// taking one at least; ones not drawn yet count a quarter of the row, and a width that is no
/// number fits one a row.
#[test]
fn ring_slots_take_as_many_a_row_as_fit() {
    // Four choosers 100 wide and the "+" 30 wide (last), 8 apart, in a row 330 wide.
    let widths = [100.0, 100.0, 100.0, 100.0, 30.0];
    let first = row_end(&widths, 0, 5, 330.0, 8.0);
    assert_eq!(first, 3, "three choosers fit 330, a fourth does not");
    assert_eq!(row_end(&widths, first, 5, 330.0, 8.0), 5, "the last chooser and the + share the next row");
    // Nothing measured yet: a quarter of the row each.
    assert_eq!(row_end(&[], 0, 9, 400.0, 0.0), 4);
    // A chooser wider than the row still takes one.
    assert_eq!(row_end(&[500.0, 100.0], 0, 2, 300.0, 8.0), 1);
    for width in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
        assert_eq!(row_end(&widths, 0, 5, width, 8.0), 1, "{width}");
    }
    assert_eq!(row_end(&widths, 5, 5, 330.0, 8.0), 5, "nothing left: an empty row ends at once");
}
