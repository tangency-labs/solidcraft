use super::*;
use egui::{Event, Modifiers, PointerButton, pos2};

const CTRL: Modifiers = Modifiers { ctrl: true, ..Modifiers::NONE };

fn button(x: f32, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos: pos2(x, 100.0), button, pressed, modifiers }
}

fn on() -> ControlClick {
    ControlClick { enabled: true, press: None }
}

#[test]
fn a_control_click_becomes_a_secondary_click() {
    let mut c = on();
    let mut events = vec![button(100.0, PointerButton::Primary, true, CTRL), button(101.0, PointerButton::Primary, false, CTRL)];
    assert!(c.remap(&mut events).is_empty());
    assert_eq!(
        events,
        vec![button(100.0, PointerButton::Secondary, true, Modifiers::NONE), button(101.0, PointerButton::Secondary, false, Modifiers::NONE)]
    );
}

#[test]
fn a_control_click_over_two_frames_waits_for_its_release() {
    let mut c = on();
    let mut first = vec![button(100.0, PointerButton::Primary, true, CTRL)];
    assert!(c.remap(&mut first).is_empty());
    assert!(first.is_empty());
    let mut second = vec![Event::PointerMoved(pos2(103.0, 100.0)), button(103.0, PointerButton::Primary, false, CTRL)];
    assert!(c.remap(&mut second).is_empty());
    assert_eq!(
        second,
        vec![
            Event::PointerMoved(pos2(103.0, 100.0)),
            button(100.0, PointerButton::Secondary, true, Modifiers::NONE),
            button(103.0, PointerButton::Secondary, false, Modifiers::NONE),
        ]
    );
}

#[test]
fn a_control_drag_stays_a_primary_drag_from_where_it_was_pressed() {
    let mut c = on();
    let mut first = vec![button(100.0, PointerButton::Primary, true, CTRL)];
    assert!(c.remap(&mut first).is_empty());
    let mut second = vec![Event::PointerMoved(pos2(130.0, 100.0)), button(130.0, PointerButton::Primary, false, CTRL)];
    let later = c.remap(&mut second);
    assert_eq!(second, vec![Event::PointerMoved(pos2(100.0, 100.0)), button(100.0, PointerButton::Primary, true, CTRL)]);
    assert_eq!(later, vec![Event::PointerMoved(pos2(130.0, 100.0)), button(130.0, PointerButton::Primary, false, CTRL)]);
    let mut third = later;
    assert!(c.remap(&mut third).is_empty(), "the drag's own events pass as they are");
    assert_eq!(third.len(), 2);
}

#[test]
fn other_clicks_and_other_platforms_are_left_alone() {
    let cmd = Modifiers { ctrl: true, mac_cmd: true, command: true, ..Modifiers::NONE };
    for events in [
        vec![button(100.0, PointerButton::Primary, true, Modifiers::NONE), button(100.0, PointerButton::Primary, false, Modifiers::NONE)],
        vec![button(100.0, PointerButton::Primary, true, cmd), button(100.0, PointerButton::Primary, false, cmd)],
        vec![button(100.0, PointerButton::Secondary, true, CTRL), button(100.0, PointerButton::Secondary, false, CTRL)],
    ] {
        let mut got = events.clone();
        assert!(on().remap(&mut got).is_empty());
        assert_eq!(got, events);
    }
    let control_click = vec![button(100.0, PointerButton::Primary, true, CTRL), button(100.0, PointerButton::Primary, false, CTRL)];
    let mut got = control_click.clone();
    assert!(ControlClick { enabled: false, press: None }.remap(&mut got).is_empty());
    assert_eq!(got, control_click);
}
