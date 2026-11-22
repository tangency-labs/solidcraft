//! What a VR controller's ray points at and acts on (`vr_pick`, docs/vr.md): the part a body
//! belongs to, what an action acts on, hiding and showing, lighting, and the joints' marks.

use crate::scenario::Harness;
use crate::vr_pick::{self, Part};
use crate::vr_settings::ActsOn;
use serde_json::json;
use solidcraft_engine::Sel;

/// A body `Top` of the design's own component, and a placement `Part` holding the body `Inner`
/// 20 mm along x: the harness, the placement's id and `Inner`'s world name.
fn body_and_placement() -> (Harness, u64, String) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Top"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Part"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"body_name":"Inner"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.settle();
    let inner = h.app.session.occurrence_bodies(&[part], true).unwrap();
    assert_eq!(inner.len(), 1, "{inner:?}");
    (h, part, inner[0].clone())
}

/// Two bodies of the design's own component, `One` at x 0..10 and `Two` at x 20..30, 10 mm cubes.
fn two_bodies() -> Harness {
    let mut h = Harness::new();
    for (name, x) in [("One", 0), ("Two", 20)] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
    }
    h.settle();
    h
}

/// The names of `parts`, in order.
fn names(parts: &[Part]) -> Vec<String> {
    parts.iter().map(|p| p.name.clone()).collect()
}

/// A body of the design's own component is its own part; a body in a placement is the
/// placement, named as the Browser names it, with the placement's bodies to light.
#[test]
fn a_body_is_its_own_part_or_its_placements() {
    let (h, part, inner) = body_and_placement();
    let top = vr_pick::part_of(&h.app, "Top");
    assert_eq!(top, Part { sel: Sel::Body { name: "Top".into() }, name: "Top".into(), bodies: vec!["Top".into()] });
    let placed = vr_pick::part_of(&h.app, &inner);
    assert_eq!(placed.sel, Sel::Occurrence { path: vec![part] });
    let row = h.app.session.occurrence_children(&[]).unwrap().into_iter().find(|c| c.path == [part]).unwrap();
    assert_eq!(placed.name, row.name, "named as its Browser row");
    assert_eq!(placed.bodies, vec![inner]);
}

/// A body name that is not in the design (a ray's stale hit) is a body part of that name: no
/// placement is made up for it.
#[test]
fn a_body_that_is_not_there_is_no_placement() {
    let (h, _, _) = body_and_placement();
    let gone = vr_pick::part_of(&h.app, "Gone");
    assert_eq!(gone.sel, Sel::Body { name: "Gone".into() });
    assert_eq!(gone.bodies, vec!["Gone".to_string()]);
}

/// Pointing at nothing, an action acts on the selection, and on nothing when nothing is
/// selected; the trigger on a part selects it, again takes it out, and on nothing clears the
/// selection.
#[test]
fn without_a_pointed_part_an_action_acts_on_the_selection() {
    let (mut h, part, inner) = body_and_placement();
    assert!(vr_pick::acted_on(&h.app, None, ActsOn::Pointed).is_empty(), "nothing selected, nothing pointed at");
    let top = vr_pick::part_of(&h.app, "Top");
    let placed = vr_pick::part_of(&h.app, &inner);
    vr_pick::toggle(&mut h.app, Some(&top));
    vr_pick::toggle(&mut h.app, Some(&placed));
    assert!(vr_pick::is_selected(&h.app, &top) && vr_pick::is_selected(&h.app, &placed));
    let on = vr_pick::acted_on(&h.app, None, ActsOn::Pointed);
    assert_eq!(on.iter().map(|p| p.sel.clone()).collect::<Vec<_>>(), vec![top.sel.clone(), Sel::Occurrence { path: vec![part] }]);
    assert_eq!(on[1].bodies, vec![inner.clone()]);
    vr_pick::toggle(&mut h.app, Some(&top));
    assert!(!vr_pick::is_selected(&h.app, &top), "the trigger again takes it out");
    assert_eq!(names(&vr_pick::acted_on(&h.app, None, ActsOn::Pointed)), names(std::slice::from_ref(&placed)));
    vr_pick::toggle(&mut h.app, None);
    assert!(h.app.session.selection.is_empty(), "the trigger on nothing clears the selection");
}

/// A part pointed at outside the selection is acted on alone; one in the selection takes the
/// whole selection with it.
#[test]
fn a_pointed_part_is_acted_on_alone_or_with_the_selection_it_is_in() {
    let (mut h, _, inner) = body_and_placement();
    let top = vr_pick::part_of(&h.app, "Top");
    let placed = vr_pick::part_of(&h.app, &inner);
    vr_pick::toggle(&mut h.app, Some(&top));
    assert_eq!(vr_pick::acted_on(&h.app, Some(&placed), ActsOn::Pointed), vec![placed.clone()], "not selected: alone");
    vr_pick::toggle(&mut h.app, Some(&placed));
    assert_eq!(
        names(&vr_pick::acted_on(&h.app, Some(&placed), ActsOn::Pointed)),
        names(&[top.clone(), placed.clone()]),
        "selected: the whole selection"
    );
    assert_eq!(names(&vr_pick::acted_on(&h.app, Some(&top), ActsOn::Pointed)), names(&[top, placed]));
}

/// Selected items that are not parts (a face, a feature, a placement that no longer exists)
/// are left out of what an action acts on; with only those selected, a pointed part is acted on
/// alone and pointing at nothing acts on nothing.
#[test]
fn selected_items_that_are_not_parts_are_left_out() {
    let (mut h, _, _) = body_and_placement();
    let top = vr_pick::part_of(&h.app, "Top");
    let face = Sel::Face { body: "Top".into(), index: 0, point: solidcore_record::Vec3::new(5.0, 5.0, 10.0) };
    h.app.session.selection = vec![face, Sel::Feature { id: 1 }, Sel::Occurrence { path: vec![9999] }];
    assert!(vr_pick::acted_on(&h.app, None, ActsOn::Pointed).is_empty());
    assert!(vr_pick::selected_parts(&h.app).is_empty());
    assert_eq!(vr_pick::acted_on(&h.app, Some(&top), ActsOn::Pointed), vec![top.clone()]);
    h.app.session.selection.push(top.sel.clone());
    assert_eq!(vr_pick::acted_on(&h.app, Some(&top), ActsOn::Pointed), vec![top], "only the part of the selection");
}

/// Nothing is not all hidden; a list is all hidden only when every part in it is, a body by its
/// own eye and a placement as a placement.
#[test]
fn all_hidden_is_true_only_when_every_part_is_hidden() {
    let (mut h, part, inner) = body_and_placement();
    let top = vr_pick::part_of(&h.app, "Top");
    let placed = vr_pick::part_of(&h.app, &inner);
    let both = [top.clone(), placed.clone()];
    assert!(!vr_pick::all_hidden(&h.app, &[]), "nothing to show");
    assert!(!vr_pick::all_hidden(&h.app, &both));
    vr_pick::hide(&mut h.app, std::slice::from_ref(&top)).unwrap();
    assert!(vr_pick::all_hidden(&h.app, std::slice::from_ref(&top)));
    assert!(!vr_pick::all_hidden(&h.app, &both), "the placement still shows");
    vr_pick::hide(&mut h.app, std::slice::from_ref(&placed)).unwrap();
    assert!(h.app.session.occurrence_hidden(&[part]), "a placement alone is hidden as a placement");
    assert!(vr_pick::all_hidden(&h.app, &both));
}

/// Placements hidden as placements show again in one step that undo takes back, as they were
/// hidden in one.
#[test]
fn a_hidden_placement_shows_again_in_one_undo_step() {
    let (mut h, part, inner) = body_and_placement();
    let placed = vr_pick::part_of(&h.app, &inner);
    vr_pick::hide(&mut h.app, std::slice::from_ref(&placed)).unwrap();
    assert!(vr_pick::all_hidden(&h.app, std::slice::from_ref(&placed)));
    vr_pick::show(&mut h.app, std::slice::from_ref(&placed)).unwrap();
    assert!(!h.app.session.occurrence_hidden(&[part]) && h.app.session.body_visible(&inner));
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(h.app.session.occurrence_hidden(&[part]), "one undo hides it again");
}

/// Show/Hide twice on a selection of a body and a placement shows all of it again: the hide
/// hides the placement's bodies one by one, so the show shows them so too (it showed only the
/// placement, which was not hidden, and left its body hidden).
#[test]
fn show_hide_twice_on_bodies_and_placements_shows_them_all_again() {
    let (mut h, _, inner) = body_and_placement();
    let both = [vr_pick::part_of(&h.app, "Top"), vr_pick::part_of(&h.app, &inner)];
    vr_pick::hide(&mut h.app, &both).unwrap();
    assert!(vr_pick::all_hidden(&h.app, &both));
    assert!(!h.app.session.body_visible(&inner));
    vr_pick::show(&mut h.app, &both).unwrap();
    assert!(h.app.session.body_visible("Top") && h.app.session.body_visible(&inner), "hidden: {:?}", h.app.session.visibility.hidden_bodies);
    assert!(!vr_pick::all_hidden(&h.app, &both[1..]));
}

/// A placement whose bodies were hidden one by one (by their eyes in the Browser) counts as
/// hidden, and showing it shows those bodies.
#[test]
fn a_placement_with_its_bodies_hidden_one_by_one_shows_them() {
    let (mut h, part, inner) = body_and_placement();
    h.app.run("browser.visibility", json!({"items":[inner], "visible":false})).unwrap();
    let placed = vr_pick::part_of(&h.app, &inner);
    assert!(vr_pick::all_hidden(&h.app, std::slice::from_ref(&placed)) && !h.app.session.occurrence_hidden(&[part]));
    vr_pick::show(&mut h.app, std::slice::from_ref(&placed)).unwrap();
    assert!(h.app.session.body_visible(&inner));
}

/// Showing parts that are all visible, or none, changes nothing and is not refused.
#[test]
fn showing_what_shows_already_changes_nothing() {
    let (mut h, _, inner) = body_and_placement();
    let placed = vr_pick::part_of(&h.app, &inner);
    let before = h.app.session.visibility.clone();
    vr_pick::show(&mut h.app, &[]).unwrap();
    vr_pick::show(&mut h.app, &[placed]).unwrap();
    assert_eq!(h.app.session.visibility, before);
}

/// Isolating nothing is refused with a reason in the user's language (the catalogue's words,
/// not English written in), and the design keeps what it shows.
#[test]
fn isolating_nothing_is_refused() {
    let (mut h, _, _) = body_and_placement();
    let before = h.app.session.visibility.clone();
    let why = vr_pick::isolate(&mut h.app, &[]).expect_err("isolating nothing is refused");
    assert_eq!(why, solidcraft_ui_text::tr("vr.nothing_to_isolate"));
    assert_eq!(h.app.session.visibility, before);
}

/// Lighting replaces only what this caller lit before: another hover (the Browser's) keeps its
/// bodies, also one both light, and lighting nothing puts out only this caller's.
#[test]
fn light_replaces_only_what_it_lit() {
    let (mut h, _, _) = body_and_placement();
    h.app.viewport.hover_bodies = vec!["Browser".into(), "Both".into()];
    let mut lit = Vec::new();
    vr_pick::light(&mut h.app, &mut lit, vec!["A".into(), "Both".into()]);
    assert_eq!(lit, vec!["A".to_string(), "Both".to_string()]);
    assert_eq!(h.app.viewport.hover_bodies, ["Browser", "Both", "A", "Both"]);
    vr_pick::light(&mut h.app, &mut lit, vec!["B".into()]);
    assert_eq!(h.app.viewport.hover_bodies, ["Browser", "Both", "B"], "the Browser's own Both stays");
    vr_pick::light(&mut h.app, &mut lit, vec!["B".into()]);
    assert_eq!(h.app.viewport.hover_bodies, ["Browser", "Both", "B"], "the same again changes nothing");
    vr_pick::light(&mut h.app, &mut lit, Vec::new());
    assert_eq!(h.app.viewport.hover_bodies, ["Browser", "Both"]);
    assert!(lit.is_empty());
}

/// What this caller lit and another hover since cleared is not taken from anyone else: a stale
/// list takes out only what is still there, once each.
#[test]
fn light_with_a_stale_list_takes_out_nothing_else() {
    let (mut h, _, _) = body_and_placement();
    let mut lit = vec!["Gone".to_string(), "Gone".to_string(), "Kept".to_string()];
    h.app.viewport.hover_bodies = vec!["Kept".into(), "Kept".into(), "Other".into()];
    vr_pick::light(&mut h.app, &mut lit, vec!["New".into()]);
    assert_eq!(h.app.viewport.hover_bodies, ["Kept", "Other", "New"]);
}

/// The ray stops at the nearest body it points at, at the face it meets, in model units.
#[test]
fn a_ray_stops_at_the_nearest_body() {
    let h = two_bodies();
    let down = vr_pick::hit(&h.app, [25.0, 5.0, 100.0], [0.0, 0.0, -3.0], &[]).unwrap();
    assert_eq!(down.body, "Two");
    assert!((down.depth - 90.0).abs() < 1e-6 && (down.point[2] - 10.0).abs() < 1e-6, "{down:?}");
    let along = vr_pick::hit(&h.app, [-50.0, 5.0, 5.0], [1.0, 0.0, 0.0], &[]).unwrap();
    assert_eq!(along.body, "One", "the nearer of the two");
    assert!((along.depth - 50.0).abs() < 1e-6, "{along:?}");
}

/// What a section cuts away is passed through: a section at x 15 cutting toward -x takes `One`
/// away, and the ray along x stops at `Two`.
#[test]
fn a_ray_passes_through_what_the_section_cuts_away() {
    let h = two_bodies();
    let hit = vr_pick::hit(&h.app, [-50.0, 5.0, 5.0], [1.0, 0.0, 0.0], &[([15.0, 0.0, 0.0], [-1.0, 0.0, 0.0])]).unwrap();
    assert_eq!(hit.body, "Two");
    assert!((hit.depth - 70.0).abs() < 1e-6, "{hit:?}");
}

/// A ray that misses, or is not a ray (no length, not numbers, a section that is not one), hits
/// nothing and does not panic.
#[test]
fn a_ray_that_misses_or_is_not_one_hits_nothing() {
    let h = two_bodies();
    let at = [-50.0, 5.0, 5.0];
    assert_eq!(vr_pick::hit(&h.app, at, [-1.0, 0.0, 0.0], &[]), None, "pointing away");
    assert_eq!(vr_pick::hit(&h.app, at, [0.0, 0.0, 0.0], &[]), None, "no length");
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(vr_pick::hit(&h.app, [bad, 5.0, 5.0], [1.0, 0.0, 0.0], &[]), None, "origin {bad}");
        assert_eq!(vr_pick::hit(&h.app, at, [1.0, bad, 0.0], &[]), None, "direction {bad}");
        assert_eq!(vr_pick::hit(&h.app, at, [1.0, 0.0, 0.0], &[([bad, 0.0, 0.0], [-1.0, 0.0, 0.0])]), None, "section at {bad}");
        assert_eq!(vr_pick::hit(&h.app, at, [1.0, 0.0, 0.0], &[([15.0, 0.0, 0.0], [bad, 0.0, 0.0])]), None, "section toward {bad}");
    }
    assert_eq!(vr_pick::hit(&h.app, at, [1.0, 0.0, 0.0], &[([15.0, 0.0, 0.0], [0.0, 0.0, 0.0])]), None, "a section with no side");
}

/// The design view's section is worked out once and handed to every ray
/// ([`vr_pick::view_section`]): a ray given it passes through what it cuts away, a ray given none
/// cuts nothing (it no longer works the view's section out again for itself), and without a
/// section there is none.
#[test]
fn the_views_section_is_handed_to_the_rays() {
    let mut h = two_bodies();
    assert!(vr_pick::view_section(&h.app).is_empty());
    let v = solidcore_record::Vec3::new;
    h.app.session.section = vec![solidcraft_engine::SectionPlane { id: 1, origin: v(15.0, 0.0, 0.0), normal: v(-1.0, 0.0, 0.0) }];
    let section = vr_pick::view_section(&h.app);
    assert_eq!(section, vec![([15.0, 0.0, 0.0], [-1.0, 0.0, 0.0])]);
    let at = [-50.0, 5.0, 5.0];
    assert_eq!(vr_pick::hit(&h.app, at, [1.0, 0.0, 0.0], &section).map(|r| r.body).as_deref(), Some("Two"), "cut as the view is");
    assert_eq!(vr_pick::hit(&h.app, at, [1.0, 0.0, 0.0], &[]).map(|r| r.body).as_deref(), Some("One"), "nothing cut");
}

/// Two top-level parts, `A` (grounded) at x 0..20 and `B` at x 30..50: the harness and `B`'s
/// placement.
fn two_parts() -> (Harness, u64) {
    let mut h = Harness::new();
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 30)], |id, p| h.app.run(id, p).unwrap());
    h.settle();
    let c = h.app.session.doc.find_component("B").unwrap();
    let b = h.app.session.doc.occurrence_of(c).unwrap().id;
    (h, b)
}

/// Explode toggles the assembly apart and back together, the design keeping its placements.
#[test]
fn explode_shows_the_assembly_apart_and_puts_it_back() {
    let (mut h, _) = two_parts();
    let doc = h.app.session.doc.clone();
    assert_eq!(vr_pick::explode(&mut h.app), Ok(true));
    assert!(!h.app.session.exploded.is_empty() && !h.app.session.moved_uncaptured(), "shown only");
    assert_eq!(vr_pick::explode(&mut h.app), Ok(false));
    assert!(h.app.session.pending_moves.is_empty() && h.app.session.exploded.is_empty());
    assert_eq!(*h.app.session.doc, *doc, "nothing added to the design");
}

/// A part moved without capturing its position: Explode is refused with a reason and changes
/// nothing (showing or leaving the exploded view would drop the move, which no undo brings
/// back), both before it is shown apart and while it is.
#[test]
fn explode_is_refused_while_moves_are_not_captured() {
    let (mut h, b) = two_parts();
    h.app.run("occurrence.move", json!({"occurrence": b, "translate": [0, 15, 0], "capture": false})).unwrap();
    assert!(h.app.session.moved_uncaptured());
    let moves = h.app.session.pending_moves.clone();
    let why = vr_pick::explode(&mut h.app).unwrap_err();
    assert!(!why.is_empty());
    assert_eq!(h.app.session.pending_moves, moves, "the move is kept");
    assert!(h.app.session.exploded.is_empty(), "not shown apart");
    // Shown apart first, then a part moved in it: putting it back together is refused too.
    let (mut h, b) = two_parts();
    assert_eq!(vr_pick::explode(&mut h.app), Ok(true));
    h.app.run("occurrence.move", json!({"occurrence": b, "translate": [0, 15, 0], "capture": false})).unwrap();
    let moves = h.app.session.pending_moves.clone();
    assert!(vr_pick::explode(&mut h.app).is_err());
    assert_eq!(h.app.session.pending_moves, moves, "the moved part stays where it was moved");
}

/// The joints' marks are kept while nothing they come from changes, and worked out again when
/// something does: the joint picked in the Browser, the glyph style, a part moved without
/// capturing (no new revision), the joint hidden, another design.
#[test]
fn the_joint_marks_follow_everything_they_come_from() {
    let (mut h, id) = jointed();
    let mut marks = vr_pick::JointMarks::default();
    let first = marks.get(&h.app).to_vec();
    assert_eq!(first, vr_pick::joint_marks(&h.app));
    assert_eq!(marks.get(&h.app), first.as_slice(), "nothing changed");
    h.app.tree.picked_joints.push(id);
    assert!(marks.get(&h.app)[0].badge.picked, "picked in the Browser");
    h.app.tree.picked_joints.clear();
    assert!(!marks.get(&h.app)[0].badge.picked, "and no longer");
    h.app.preferences.joint_symbols = 3;
    assert_eq!(marks.get(&h.app)[0].badge.style, 3, "another glyph style");
    let a = h.app.session.doc.assembly.joints[0].a.occurrence;
    let before = marks.get(&h.app)[0].at;
    let moved = h.app.run("occurrence.move", json!({"occurrence": a, "translate": [0, 0, 7], "capture": false, "move_grounded": true}));
    assert!(moved.is_ok(), "{moved:?}");
    let after = marks.get(&h.app)[0].at;
    assert_eq!(after, vr_pick::joint_marks(&h.app)[0].at, "as worked out afresh");
    assert_ne!(after, before, "moved with its part");
    h.app.run("browser.visibility", json!({"joints":[id], "visible":false})).unwrap();
    assert!(marks.get(&h.app).is_empty(), "hidden");
    let other = Harness::new();
    assert!(marks.get(&other.app).is_empty(), "another design has none");
}

/// Many joints' marks are found by id, one each, in the order the window draws them: a design
/// with many joints has a mark for each, none twice.
#[test]
fn every_drawn_joint_has_one_mark() {
    let (mut h, _) = jointed();
    let j = h.app.session.doc.assembly.joints[0].clone();
    // Copies of the joint under new ids (the same two parts): the marks find each by its id.
    let mut doc = (*h.app.session.doc).clone();
    for k in 1..200u64 {
        doc.assembly.joints.push(solidcraft_engine::doc::joints::Joint { id: j.id + 1000 + k, ..j.clone() });
    }
    h.app.session.doc = std::sync::Arc::new(doc);
    h.app.session.revision += 1;
    let marks = vr_pick::joint_marks(&h.app);
    let ids: Vec<u64> = marks.iter().map(|m| m.id).collect();
    let expected: Vec<u64> = h.app.session.doc.assembly.joints.iter().map(|j| j.id).collect();
    assert_eq!(ids, expected);
}

/// Two grounded parts and a rigid joint between them: the harness and the joint's id.
fn jointed() -> (Harness, u64) {
    let mut h = Harness::new();
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 30)], |id, p| h.app.run(id, p).unwrap());
    let occ = |h: &Harness, name: &str| {
        let c = h.app.session.doc.find_component(name).unwrap();
        h.app.session.doc.occurrence_of(c).unwrap().id
    };
    let (a, b) = (occ(&h, "A"), occ(&h, "B"));
    h.app
        .run("joint.create", json!({"type": "rigid", "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}))
        .unwrap();
    h.settle();
    let id = h.app.session.doc.assembly.joints[0].id;
    (h, id)
}

/// A shown joint has a mark at its origin (on A's top face, where it was made), of its type and
/// the window's glyph style; a design without joints has none.
#[test]
fn a_shown_joint_has_a_mark_at_its_origin() {
    assert!(vr_pick::joint_marks(&two_bodies().app).is_empty(), "no joints, no marks");
    let (h, id) = jointed();
    let marks = vr_pick::joint_marks(&h.app);
    assert_eq!(marks.len(), 1, "{marks:?}");
    let m = &marks[0];
    assert_eq!(m.id, id);
    let joint = &h.app.session.doc.assembly.joints[0];
    assert_eq!((m.badge.kind, m.badge.style), (joint.kind, h.app.preferences.joint_symbols));
    assert!(!m.badge.picked && !m.badge.suppressed);
    assert!((m.at[2] - 10.0).abs() < 1e-6, "on A's top face: {:?}", m.at);
}

/// Only the joints the window draws have marks: a joint hidden by its eye, or by the Joints
/// folder's, has none, until it is picked in the Browser, which draws it picked.
#[test]
fn only_the_joints_the_window_draws_have_marks() {
    let (mut h, id) = jointed();
    h.app.run("browser.visibility", json!({"joints":[id], "visible":false})).unwrap();
    assert!(vr_pick::joint_marks(&h.app).is_empty(), "hidden by its eye");
    h.app.tree.picked_joints.push(id);
    let marks = vr_pick::joint_marks(&h.app);
    assert_eq!(marks.len(), 1);
    assert!(marks[0].badge.picked, "a picked joint is drawn, picked");
    h.app.tree.picked_joints.clear();
    h.app.run("browser.visibility", json!({"joints":[id], "visible":true})).unwrap();
    h.app.run("browser.visibility", json!({"tree_folders":["c0/joints"], "visible":false})).unwrap();
    assert!(vr_pick::joint_marks(&h.app).is_empty(), "hidden by the Joints folder");
}

/// `bodies_of` lists each body once, in the order the parts give them.
#[test]
fn bodies_of_lists_each_body_once_in_order() {
    let part = |bodies: &[&str]| Part {
        sel: Sel::Body { name: String::new() },
        name: String::new(),
        bodies: bodies.iter().map(|b| b.to_string()).collect(),
    };
    assert_eq!(vr_pick::bodies_of(&[part(&["B", "A"]), part(&["A", "C", "B"])]), ["B", "A", "C"]);
    assert!(vr_pick::bodies_of(std::iter::empty::<&Part>()).is_empty());
}

/// A design `Top`, holding the placement `Bay` with its own body `Cell` and, inside it, the
/// placement `Contact` with the body `Pin`; the root active. The harness, `Bay`'s and
/// `Contact`'s components and placements, and `Cell`'s and `Pin`'s world names.
fn nested() -> (Harness, [u64; 2], [u64; 2], [String; 2]) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Top"})).unwrap();
    let bay = h.app.run("component.create", json!({"name":"Bay"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"body_name":"Cell"})).unwrap();
    let contact = h.app.run("component.create", json!({"name":"Contact"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[40,0,0],"body_name":"Pin"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.settle();
    let id = |v: &serde_json::Value, k: &str| v[k].as_u64().unwrap();
    let (components, occurrences) = ([id(&bay, "component"), id(&contact, "component")], [id(&bay, "occurrence"), id(&contact, "occurrence")]);
    let pin = h.app.session.occurrence_bodies(&occurrences, true).unwrap();
    let cell: Vec<String> = h.app.session.occurrence_bodies(&occurrences[..1], true).unwrap().into_iter().filter(|b| !pin.contains(b)).collect();
    assert_eq!((cell.len(), pin.len()), (1, 1), "{cell:?} {pin:?}");
    (h, components, occurrences, [cell[0].clone(), pin[0].clone()])
}

/// The level a body is picked at: one inside the active placement, the body itself when it is
/// the active component's own, and as with the root active when the body is not inside the
/// active placement or that is not known.
#[test]
fn a_pick_is_one_level_inside_the_active_placement() {
    use vr_pick::picked_at;
    assert_eq!(picked_at(&[], Some(&[])), None, "a root body is itself");
    assert_eq!(picked_at(&[1, 2, 3], Some(&[])), Some(vec![1]), "the root active: the top-level placement");
    assert_eq!(picked_at(&[1, 2, 3], Some(&[1])), Some(vec![1, 2]));
    assert_eq!(picked_at(&[1, 2, 3], Some(&[1, 2])), Some(vec![1, 2, 3]));
    assert_eq!(picked_at(&[1, 2, 3], Some(&[1, 2, 3])), None, "the active component's own body is itself");
    assert_eq!(picked_at(&[1, 2, 3], Some(&[4])), Some(vec![1]), "outside the active placement: as with the root");
    assert_eq!(picked_at(&[1, 2, 3], Some(&[1, 2, 3, 4])), Some(vec![1]), "a placement inside the body's is not around it");
    assert_eq!(picked_at(&[1, 2, 3], None), Some(vec![1]));
    assert_eq!(picked_at(&[], Some(&[4])), None);
}

/// Activating a subassembly picks the parts it holds: its own bodies, and the placements directly
/// inside it; activating one of those goes a level deeper again. The root active picks the
/// top-level placement, and a body outside the active component is picked as with the root.
#[test]
fn activating_a_subassembly_picks_what_it_holds() {
    let (mut h, [bay_c, contact_c], [bay, contact], [cell, pin]) = nested();
    let sel = |h: &Harness, body: &str| vr_pick::part_of(&h.app, body).sel;
    assert_eq!(sel(&h, &cell), Sel::Occurrence { path: vec![bay] });
    assert_eq!(sel(&h, &pin), Sel::Occurrence { path: vec![bay] }, "the root active: the top-level placement");
    assert_eq!(sel(&h, "Top"), Sel::Body { name: "Top".into() });

    h.app.run("component.activate", json!({"component": bay_c, "occurrence": bay, "path": [bay]})).unwrap();
    h.settle();
    assert_eq!(sel(&h, &cell), Sel::Body { name: cell.clone() }, "the active component's own body");
    let deeper = vr_pick::part_of(&h.app, &pin);
    assert_eq!(deeper.sel, Sel::Occurrence { path: vec![bay, contact] });
    let row = h.app.session.occurrence_children(&[bay]).unwrap().into_iter().find(|c| c.path == [bay, contact]).unwrap();
    assert_eq!((deeper.name, deeper.bodies), (row.name, vec![pin.clone()]), "named as its Browser row");
    assert_eq!(sel(&h, "Top"), Sel::Body { name: "Top".into() }, "outside the active component: as with the root");

    h.app.run("component.activate", json!({"component": contact_c, "occurrence": contact, "path": [bay, contact]})).unwrap();
    h.settle();
    assert_eq!(sel(&h, &pin), Sel::Body { name: pin.clone() });
    assert_eq!(sel(&h, &cell), Sel::Occurrence { path: vec![bay] }, "a body around the active component is outside it");

    // Activated by the component alone: its placement on the way to the body.
    h.app.run("component.activate", json!({"component": bay_c})).unwrap();
    h.settle();
    if h.app.session.active_occurrence_path.is_none() {
        assert_eq!(sel(&h, &cell), Sel::Body { name: cell.clone() });
        assert_eq!(sel(&h, &pin), Sel::Occurrence { path: vec![bay, contact] });
    }
}

/// A selected placement is named as its own Browser row, whatever level the pick is at.
#[test]
fn a_selected_placement_keeps_its_own_name() {
    let (mut h, _, [bay, contact], _) = nested();
    h.app.session.selection = vec![Sel::Occurrence { path: vec![bay, contact] }];
    let row = h.app.session.occurrence_children(&[bay]).unwrap().into_iter().find(|c| c.path == [bay, contact]).unwrap();
    assert_eq!(names(&vr_pick::selected_parts(&h.app)), vec![row.name]);
}

/// Show all shows every hidden body and placement again, in one undo step, and leaves a hidden
/// sketch and a hidden joint hidden.
#[test]
fn show_all_shows_the_parts_and_leaves_sketches_and_joints_alone() {
    let (mut h, joint) = jointed();
    h.app.run("sketch.create", json!({"plane": "XY", "offset": 50, "name": "Note"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    let sketch = h.app.session.model.state().sketches.last().unwrap().feature;
    h.app.run("browser.visibility", json!({"joints": [joint], "visible": false})).unwrap();
    h.app.run("browser.visibility", json!({"items": ["Note"], "visible": false})).unwrap();
    let a = h.app.session.doc.find_component("A").unwrap();
    let path = vec![h.app.session.doc.occurrence_of(a).unwrap().id];
    let body = h.app.session.occurrence_bodies(&path, true).unwrap()[0].clone();
    let placed = vr_pick::part_of(&h.app, &body);
    vr_pick::hide(&mut h.app, &[placed]).unwrap();
    assert!(h.app.session.occurrence_hidden(&path));
    assert!(h.app.session.visibility.hidden_sketches.contains(&sketch) && h.app.session.visibility.hidden_joints.contains(&joint));
    let undo = h.app.session.undo.len();
    vr_pick::show_all(&mut h.app).unwrap();
    assert!(!h.app.session.occurrence_hidden(&path) && h.app.session.visibility.hidden_bodies.is_empty(), "the parts show");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert!(h.app.session.visibility.hidden_sketches.contains(&sketch), "the sketch stays hidden");
    assert!(h.app.session.visibility.hidden_joints.contains(&joint), "the joint stays hidden");
}

/// Acting on the selection first: with parts selected, an action acts on them whatever the ray
/// points at, a part in the selection or out of it; with none selected (or only faces and
/// features), on the part pointed at, and on nothing when nothing is pointed at either.
#[test]
fn acting_on_the_selection_first_ignores_what_is_pointed_at() {
    let (mut h, _, inner) = body_and_placement();
    let top = vr_pick::part_of(&h.app, "Top");
    let placed = vr_pick::part_of(&h.app, &inner);
    let on = |h: &Harness, pointed: Option<&Part>| names(&vr_pick::acted_on(&h.app, pointed, ActsOn::Selection));
    assert!(on(&h, None).is_empty(), "nothing selected, nothing pointed at");
    assert_eq!(on(&h, Some(&placed)), names(std::slice::from_ref(&placed)), "nothing selected: the part pointed at");
    vr_pick::toggle(&mut h.app, Some(&top));
    assert_eq!(on(&h, Some(&placed)), names(std::slice::from_ref(&top)), "the selection, not the part pointed at");
    assert_eq!(on(&h, Some(&top)), names(std::slice::from_ref(&top)));
    assert_eq!(on(&h, None), names(std::slice::from_ref(&top)));
    h.app.session.selection = vec![Sel::Feature { id: 1 }];
    assert_eq!(on(&h, Some(&placed)), names(&[placed]), "a selection of no parts: the part pointed at");
}
