use super::*;

fn step(label: &str) -> Snapshot {
    Snapshot {
        label: label.to_string(),
        doc: Default::default(),
        active_sketch: None,
        pending_moves: Default::default(),
        visibility: Default::default(),
    }
}

fn labels(steps: &[Snapshot]) -> Vec<&str> {
    steps.iter().map(|s| s.label.as_str()).collect()
}

/// A history of `n` steps named `old 0`, `old 1` and on.
fn with(n: usize) -> History {
    let mut h = History::default();
    for i in 0..n {
        h.push(step(&format!("old {i}")));
    }
    h
}

#[test]
fn the_cap_keeps_the_newest_200_and_a_push_clears_redo() {
    let mut h = with(MAX_UNDO + 5);
    assert_eq!(h.len(), MAX_UNDO);
    assert_eq!(h.steps().first().map(|s| s.label.as_str()), Some("old 5"));
    assert_eq!(h.last().map(|s| s.label.as_str()), Some("old 204"));
    let back = h.take_undo().unwrap();
    h.undone(step("after"));
    assert_eq!((back.label.as_str(), h.len(), labels(h.redo_steps())), ("old 204", MAX_UNDO - 1, vec!["after"]));
    h.push(step("new"));
    assert!(h.redo_steps().is_empty());
    assert_eq!(h.len(), MAX_UNDO);
}

#[test]
fn undo_and_redo_move_one_step_across() {
    let mut h = with(2);
    assert!(h.take_redo().is_none());
    let to = h.take_undo().unwrap();
    h.undone(step("state after old 1"));
    assert_eq!((to.label.as_str(), labels(h.steps()), labels(h.redo_steps())), ("old 1", vec!["old 0"], vec!["state after old 1"]));
    let to = h.take_redo().unwrap();
    h.redone(step("old 1 again"));
    assert_eq!((to.label.as_str(), labels(h.steps()), h.redo_steps().len()), ("state after old 1", vec!["old 0", "old 1 again"], 0));
    while h.take_undo().is_some() {
        h.undone(step("x"));
    }
    assert!(h.is_empty() && h.redo_steps().len() == 2);
}

/// An undo and a redo put a step back as it was: a mark taken before them still finds nothing
/// pushed since, so a gesture begun then doesn't fold the redone step.
#[test]
fn undo_then_redo_keeps_a_steps_number() {
    let mut h = with(3);
    let mark = h.mark();
    assert!(h.take_undo().is_some());
    h.undone(step("state after old 2"));
    assert!(h.take_redo().is_some());
    h.redone(step("old 2 again"));
    assert_eq!(h.steps_since(mark), 0);
    assert_eq!(h.fold_since(mark), 0);
    assert_eq!(labels(h.steps()), ["old 0", "old 1", "old 2 again"]);
}

#[test]
fn a_gesture_folds_into_its_first_step() {
    let mut h = with(3);
    let mark = h.mark();
    for frame in 0..4 {
        h.push(step(&format!("frame {frame}")));
        assert_eq!(h.fold_since(mark), if frame == 0 { 1 } else { 2 });
    }
    assert_eq!(labels(h.steps()), ["old 0", "old 1", "old 2", "frame 0"]);
    h.name_last("Drag");
    assert_eq!(h.last().map(|s| s.label.as_str()), Some("Drag"));
    // Nothing pushed since: nothing folds, nothing goes.
    assert_eq!(h.fold_since(h.mark()), 0);
    assert_eq!(h.len(), 4);
}

/// At a full history a fold by length folded nothing, and a gesture lost an old step every frame.
#[test]
fn a_gesture_on_a_full_history_is_one_step_and_costs_one_old_step() {
    let mut h = with(MAX_UNDO);
    let mark = h.mark();
    for frame in 0..3 {
        h.push(step(&format!("frame {frame}")));
        h.fold_since(mark);
    }
    assert_eq!(h.len(), MAX_UNDO);
    assert_eq!(h.steps().first().map(|s| s.label.as_str()), Some("old 1"), "only the oldest went");
    assert_eq!(h.last().map(|s| s.label.as_str()), Some("frame 0"));
    // A gesture's frames all at once, folded at the end.
    let mut h = with(MAX_UNDO);
    let mark = h.mark();
    for frame in 0..5 {
        h.push(step(&format!("frame {frame}")));
    }
    assert_eq!(h.fold_since(mark), 5);
    assert_eq!((h.len(), h.steps().first().map(|s| s.label.as_str())), (MAX_UNDO, Some("old 1")));
}

/// A gesture folded once at its end (as a blank sketch's Escape is), after more frames than the
/// history keeps pushed out: what it can't give back is lost, oldest first, and nothing else.
#[test]
fn a_long_gesture_folded_once_on_a_full_history_loses_what_it_cannot_give_back() {
    let mut h = with(MAX_UNDO);
    let mark = h.mark();
    for frame in 0..20 {
        h.push(step(&format!("frame {frame}")));
    }
    assert_eq!(h.fold_since(mark), 20);
    // Frame 0 pushed out old 0 and keeps its place; frames 1 to 19 pushed out old 1 to old 19,
    // of which the last 16 come back.
    assert_eq!(h.len(), MAX_UNDO - 3);
    assert_eq!(h.steps().first().map(|s| s.label.as_str()), Some("old 4"));
    assert_eq!(h.last().map(|s| s.label.as_str()), Some("frame 0"));
    assert!(h.steps().windows(2).all(|w| w[0].label != w[1].label));
}

#[test]
fn dropping_a_runs_steps_gives_back_what_they_pushed_out() {
    let mut h = with(MAX_UNDO);
    let mark = h.mark();
    h.push(step("a"));
    h.push(step("b"));
    assert_eq!(h.steps_since(mark), 2);
    h.drop_since(mark);
    assert_eq!(h.len(), MAX_UNDO);
    assert_eq!((h.steps().first().map(|s| s.label.as_str()), h.last().map(|s| s.label.as_str())), (Some("old 0"), Some("old 199")));
    // Without a full history nothing was pushed out.
    let mut h = with(3);
    let mark = h.mark();
    h.push(step("a"));
    h.drop_since(mark);
    assert_eq!(labels(h.steps()), ["old 0", "old 1", "old 2"]);
}

#[test]
fn only_what_the_removed_steps_pushed_out_comes_back() {
    let mut h = with(MAX_UNDO);
    // Ordinary commands push out old 0 and old 1 for good.
    h.push(step("x"));
    h.push(step("y"));
    let mark = h.mark();
    h.push(step("a"));
    h.push(step("b"));
    h.fold_since(mark);
    assert_eq!(h.steps().first().map(|s| s.label.as_str()), Some("old 3"), "b's push-out came back, a's stays out");
    assert_eq!(h.len(), MAX_UNDO);
}

#[test]
fn append_takes_a_computed_copys_steps_capped_and_its_redo() {
    let mut h = with(MAX_UNDO - 1);
    let mut computed = History::default();
    computed.push(step("c0"));
    computed.push(step("c1"));
    computed.push(step("c2"));
    let to = computed.take_undo().unwrap();
    computed.undone(to);
    h.append(computed);
    assert_eq!(h.len(), MAX_UNDO);
    assert_eq!(h.steps().first().map(|s| s.label.as_str()), Some("old 1"));
    assert_eq!(h.last().map(|s| s.label.as_str()), Some("c1"));
    assert_eq!(labels(h.redo_steps()), ["c2"]);
    // A mark taken now still folds what is pushed after it, and a redo keeps the order.
    let to = h.take_redo().unwrap();
    h.redone(to);
    let mark = h.mark();
    h.push(step("d"));
    h.push(step("e"));
    assert_eq!(h.fold_since(mark), 2);
    assert_eq!(h.last().map(|s| s.label.as_str()), Some("d"));
}

#[test]
fn a_mark_from_before_a_clear_folds_only_what_came_after() {
    let mut h = with(5);
    let mark = h.mark();
    h.push(step("a"));
    h.clear();
    assert!(h.is_empty() && h.redo_steps().is_empty());
    assert_eq!(h.fold_since(mark), 0);
    h.push(step("b"));
    h.push(step("c"));
    assert_eq!(h.fold_since(mark), 2);
    assert_eq!(labels(h.steps()), ["b"]);
}

/// A new or opened design keeps nothing of the old one: a clear lets go of the steps kept to give
/// back too (up to 16 whole designs held for nothing), and dropping steps afterwards gives
/// nothing from before the clear back.
#[test]
fn a_clear_lets_go_of_the_steps_kept_to_give_back() {
    let mut h = with(MAX_UNDO);
    h.push(step("a"));
    assert_eq!(h.pushed_out.len(), 1, "a pushed old 0 out");
    h.clear();
    assert!(h.pushed_out.is_empty());
    let mark = h.mark();
    h.push(step("b"));
    h.drop_since(mark);
    assert!(h.is_empty(), "{:?}", labels(h.steps()));
}

#[test]
fn save_as_changes_every_step() {
    let mut h = with(MAX_UNDO + 1);
    let to = h.take_undo().unwrap();
    h.undone(to);
    h.for_each_step(|s| s.label.push('!'));
    assert!(h.steps().iter().chain(h.redo_steps()).all(|s| s.label.ends_with('!')));
    // A step pushed out before Save As and given back after it carries the change too: a
    // gesture's first frame pushes old 0 out, the design is saved as, and the gesture is dropped.
    let mut h = with(MAX_UNDO);
    let mark = h.mark();
    h.push(step("frame"));
    h.for_each_step(|s| s.label.push('!'));
    h.drop_since(mark);
    assert_eq!(h.steps().first().map(|s| s.label.as_str()), Some("old 0!"));
    assert!(h.steps().iter().all(|s| s.label.ends_with('!')), "{:?}", labels(h.steps()));
}
