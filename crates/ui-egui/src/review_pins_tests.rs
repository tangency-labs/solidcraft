//! Review pins in the running window: Mark this and M, the card, the list, a write by another
//! program, and the design and its undo history left alone.

use super::*;
use crate::scenario::Harness;
use serde_json::json;

/// A saved 40 × 30 × 10 box (so its markups have a file beside it), seen from Home, Review on.
fn reviewed_box() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10})).unwrap();
    h.app.session.path = Some(h.dir.join("plate.sc").to_string_lossy().into_owned());
    h.app.run("view.home", json!({})).unwrap();
    h.frames(30);
    h.app.run("view.review", json!({"visible": true})).unwrap();
    h.frames(3);
    h
}

/// The box's top face in the middle, on the screen.
fn top_middle(h: &Harness) -> Pos2 {
    let st = h.app.session.world_state().unwrap();
    let b = st.bodies.first().unwrap().body.exact_bounds().unwrap();
    let c = b.center();
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    proj.to_screen(Vec3::new(c.x, c.y, b.max.z)).unwrap()
}

fn listed(h: &Harness) -> Vec<Row> {
    h.app.review.listing.as_ref().map(|l| l.markups.clone()).unwrap_or_default()
}

/// Press M over `at`, write `note` and Add.
fn mark(h: &mut Harness, at: Pos2, note: &str) {
    h.step(&json!({"move": [at.x, at.y]})).unwrap();
    h.step(&json!({"key": "M"})).unwrap();
    h.frames(3);
    assert!(h.app.review.draft.is_some(), "M made no draft");
    h.app.review.draft.as_mut().unwrap().text = note.into();
    h.frames(2);
    crate::test_support::click(h, "review:add");
    h.frames(3);
}

#[test]
fn a_pin_is_convex_and_comes_to_a_point_at_its_tip() {
    let tip = pos2(100.0, 200.0);
    let pts = teardrop(tip, 12.0);
    assert_eq!(pts[0], tip);
    assert!(pts.iter().all(|p| p.y <= tip.y + 1e-3), "the tip is the lowest point");
    // Convex: every turn goes the same way.
    let n = pts.len();
    let turns: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
            (b - a).x * (c - b).y - (b - a).y * (c - b).x
        })
        .collect();
    assert!(turns.iter().all(|t| *t >= -1e-3) || turns.iter().all(|t| *t <= 1e-3), "{turns:?}");
    let top = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    assert!((tip.y - top - 12.0 * 2.65).abs() < 0.5, "a head of radius 12 above a point 1.65 radii down");
}

#[test]
fn a_card_stands_beside_its_pin_inside_the_view_and_clear_of_the_list() {
    let view = Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 900.0));
    let list = Some(Rect::from_min_max(pos2(1260.0, 120.0), pos2(1590.0, 700.0)));
    let size = vec2(360.0, 400.0);
    // Room on the left: the card is left of the pin.
    let at = card_place(view, list, size, Some(pos2(800.0, 400.0)));
    assert!(at.x + size.x <= 800.0 - 10.0, "{at:?}");
    // At the left edge it goes to the pin's right.
    let at = card_place(view, list, size, Some(pos2(100.0, 400.0)));
    assert!(at.x >= 100.0 + 10.0, "{at:?}");
    // It never covers the list.
    let at = card_place(view, list, size, Some(pos2(1200.0, 400.0)));
    assert!(at.x + size.x <= 1260.0, "{at:?}");
    // Near the bottom it keeps inside the view.
    let at = card_place(view, list, size, Some(pos2(800.0, 880.0)));
    assert!(at.y + size.y <= 900.0, "{at:?}");
    // No pin on the screen: beside the list, at its top.
    let at = card_place(view, list, size, None);
    assert!(at.x + size.x <= 1260.0 && (at.y - 120.0).abs() < 1.0, "{at:?}");
}

#[test]
fn m_marks_a_face_and_the_card_adds_it_without_touching_the_design() {
    let mut h = reviewed_box();
    let (revision, undo, dirty) = (h.app.session.revision, h.app.session.undo.len(), h.app.session.is_dirty());
    let selection = h.app.session.selection.clone();
    let at = top_middle(&h);
    mark(&mut h, at, "Chamfer this edge 1 mm");
    let rows = listed(&h);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let m = &rows[0];
    assert_eq!(m.note, "Chamfer this edge 1 mm");
    assert_eq!(m.status, "open");
    assert_eq!(m.anchor["kind"], "face", "the face was found on the exact geometry: {}", m.anchor);
    assert_eq!(m.found.state, "anchored");
    assert!(m.view.is_some(), "the view it was marked from is kept");
    assert_eq!(m.by, me());
    assert_eq!(h.app.review.chosen, Some(m.id), "its card stays open");
    assert!(crate::scenario::handle_at(&format!("review:pin:{}", m.id)).is_some(), "its pin is drawn");
    assert!(h.app.review.drawn.card.is_some() && h.app.review.drawn.list.is_some());
    assert_eq!(h.app.session.revision, revision, "the design did not change");
    assert_eq!(h.app.session.undo.len(), undo, "no undo step");
    assert_eq!(h.app.session.is_dirty(), dirty);
    assert_eq!(h.app.session.selection, selection, "marking selects nothing");
    assert!(h.app.dialog.is_none() && h.app.tool.is_none(), "M did not start Move/Copy");
    // With the pointer over the open card, M is Mark this: no pin at a place the pointer is not
    // over, and the next click on the model places it.
    let card = h.app.review.drawn.card.unwrap();
    h.step(&json!({"move": [card.center().x, card.center().y]})).unwrap();
    h.step(&json!({"key": "M"})).unwrap();
    h.frames(2);
    assert!(h.app.review.placing && h.app.review.draft.is_none(), "M over the card waits for a click on the model");
    let beside = at + vec2(30.0, 25.0);
    h.step(&json!({"click": [beside.x, beside.y]})).unwrap();
    h.frames(3);
    assert!(!h.app.review.placing && h.app.review.draft.as_ref().is_some_and(|d| d.at.is_some()), "the click placed the new pin");
}

/// A place on the screen where the first thing `pick` meets is wanted by `want`.
fn screen_where(h: &Harness, from: Vec3, to: Vec3, want: impl Fn(&crate::viewport::Hit) -> bool) -> Option<Pos2> {
    let rect = h.app.viewport.rect.unwrap();
    let proj = crate::viewport::projection(&h.app, rect);
    (1..20)
        .filter_map(|i| proj.to_screen(from + (to - from) * (f64::from(i) / 20.0)))
        .find(|p| rect.contains(*p) && !crate::shell::covered(&h.app, *p) && crate::viewport::pick(&h.app, &proj, *p).first().is_some_and(&want))
}

#[test]
fn an_edge_marks_an_edge_empty_space_marks_the_view_and_go_there_reads_degrees() {
    let mut h = reviewed_box();
    h.app.preferences.look_at_anim = false;
    let b = h.app.session.world_state().unwrap().bodies.first().unwrap().body.exact_bounds().unwrap();
    // The top front edge: an edge markup, and Select picks that edge, not its body.
    let (front_left, front_right) = (Vec3::new(b.min.x, b.min.y, b.max.z), Vec3::new(b.max.x, b.min.y, b.max.z));
    let edge =
        screen_where(&h, front_left, front_right, |hit| matches!(hit, crate::viewport::Hit::Edge { .. })).expect("the top front edge on the screen");
    mark(&mut h, edge, "Break this edge");
    let m = listed(&h).into_iter().max_by_key(|r| r.id).unwrap();
    assert_eq!(m.anchor["kind"], "edge", "{}", m.anchor);
    assert_eq!(m.found.state, "anchored", "marked on the edge, found on it");
    select(&mut h.app, &m);
    assert!(matches!(h.app.session.selection.as_slice(), [solidcraft_engine::Sel::Edge { .. }]), "{:?}", h.app.session.selection);
    // Empty space with that edge still selected: the markup is on the view, as its card said.
    let rect = h.app.viewport.rect.unwrap();
    let proj = crate::viewport::projection(&h.app, rect);
    let empty = (1..10)
        .flat_map(|i| (1..10).map(move |j| rect.min + vec2(rect.width() * i as f32 / 10.0, rect.height() * j as f32 / 10.0)))
        .find(|p| !crate::shell::covered(&h.app, *p) && crate::viewport::pick(&h.app, &proj, *p).is_empty())
        .expect("empty space on the screen");
    mark(&mut h, empty, "This whole view");
    let v = listed(&h).into_iter().max_by_key(|r| r.id).unwrap();
    assert_eq!(v.anchor["kind"], "view", "{}", v.anchor);
    // Its camera is kept in degrees, as the set's format says, and Go there turns back to it.
    let was = h.app.cam;
    let seen = v.view.unwrap();
    assert!((seen.yaw - was.yaw.to_degrees()).abs() < 1e-6 && (seen.pitch - was.pitch.to_degrees()).abs() < 1e-6, "{seen:?} {was:?}");
    h.app.cam.yaw += 1.0;
    h.app.cam.pitch -= 0.3;
    go_there(&mut h.app, &v);
    assert!((h.app.cam.yaw - was.yaw).abs() < 1e-9 && (h.app.cam.pitch - was.pitch).abs() < 1e-9, "{:?} {was:?}", h.app.cam);
}

#[test]
fn select_finds_a_renamed_body_and_a_lost_screenshot_frame_gives_the_list_back() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "Thicker");
    let m = listed(&h).into_iter().max_by_key(|r| r.id).unwrap();
    let name = h.app.session.world_state().unwrap().bodies.first().unwrap().name.clone();
    h.app.run("body.rename", json!({"body": name, "name": "Lid"})).unwrap();
    h.frames(3);
    h.app.run("select.clear", json!({})).unwrap();
    select(&mut h.app, &m);
    assert!(
        matches!(h.app.session.selection.as_slice(), [solidcraft_engine::Sel::Face { body, .. }] if body == "Lid"),
        "{:?}",
        h.app.session.selection
    );
    // A screenshot whose frame never comes is given up, and the list and card come back.
    let view = h.app.viewport.rect.unwrap();
    app_shoot(&mut h, view, m.id);
    h.frames(1);
    assert!(h.app.review.drawn.list.is_none(), "the list is hidden for the picture");
    h.app.shots.queued.clear();
    h.app.review.shot.as_mut().unwrap().until = crate::now_ms() - 1.0;
    h.frames(2);
    assert!(h.app.review.shot.is_none() && h.app.review.drawn.list.is_some(), "the list is back");
}

/// Ask the window for a Review screenshot, as Screenshot on the card does.
fn app_shoot(h: &mut Harness, view: Rect, markup: u64) {
    let ctx = h.ctx.clone();
    shoot(&mut h.app, &ctx, view, markup);
}

#[test]
fn cancel_and_esc_leave_nothing_in_the_set() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    h.step(&json!({"move": [at.x, at.y]})).unwrap();
    h.step(&json!({"key": "M"})).unwrap();
    h.frames(3);
    crate::test_support::click(&mut h, "review:cancel");
    assert!(h.app.review.draft.is_none());
    h.step(&json!({"key": "M"})).unwrap();
    h.frames(2);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(3);
    assert!(h.app.review.draft.is_none());
    assert!(listed(&h).is_empty());
    let set = solidcraft_engine::io::markups::beside(h.app.session.path.as_deref().unwrap());
    assert!(!std::path::Path::new(&set).exists(), "no set file was made");
}

#[test]
fn a_pin_click_opens_its_card_and_selects_nothing_under_it() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "Make it 12 mm thick");
    let id = listed(&h)[0].id;
    crate::test_support::click(&mut h, "review:card:close");
    assert_eq!(h.app.review.chosen, None);
    let selection = h.app.session.selection.clone();
    crate::test_support::click(&mut h, &format!("review:pin:{id}"));
    assert_eq!(h.app.review.chosen, Some(id));
    assert_eq!(h.app.session.selection, selection, "the face under the pin was not picked");
}

#[test]
fn reply_resolve_and_reopen_from_the_card_and_an_agents_write_shows() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "Round the outer corners R5");
    let id = listed(&h)[0].id;
    h.app.review.reply = "Which corners?".into();
    h.frames(1);
    crate::test_support::click(&mut h, "review:send");
    assert_eq!(listed(&h)[0].thread.len(), 2);
    crate::test_support::click(&mut h, "review:resolve");
    match h.app.review.ask.as_mut() {
        Some(Ask::Resolve(why)) => *why = "Fillet 2, 5 mm".into(),
        other => panic!("{other:?}"),
    }
    h.frames(1);
    crate::test_support::click(&mut h, "review:close_go");
    let m = &listed(&h)[0];
    assert_eq!(m.status, "resolved");
    assert_eq!(m.look(), Look::Done);
    crate::test_support::click(&mut h, "review:reopen");
    assert_eq!(listed(&h)[0].status, "open");
    // Another program (an agent through the CLI) closes it on the disk: the window shows it.
    let set = solidcraft_engine::io::markups::beside(h.app.session.path.as_deref().unwrap());
    let mut agent = solidcraft_engine::Session::default();
    agent.execute("markup.reject", &json!({"set": set, "markup": id, "reason": "It is a fillet already"})).unwrap();
    for _ in 0..90 {
        h.frame();
        if listed(&h)[0].status == "rejected" {
            break;
        }
    }
    assert_eq!(listed(&h)[0].status, "rejected", "the window looked at the set again");
    assert_eq!(listed(&h)[0].outcome.as_ref().unwrap().by, "agent");
}

#[test]
fn the_filter_decides_the_pins_and_the_eye_hides_them() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "First");
    let id = listed(&h)[0].id;
    crate::test_support::click(&mut h, "review:card:close");
    let pin = format!("review:pin:{id}");
    assert!(crate::scenario::handle_at(&pin).is_some());
    crate::test_support::click(&mut h, "review:filter:done");
    assert!(crate::scenario::handle_at(&pin).is_none(), "an open pin is not drawn under Done");
    crate::test_support::click(&mut h, "review:filter:all");
    assert!(crate::scenario::handle_at(&pin).is_some());
    crate::test_support::click(&mut h, "review:eye");
    assert!(crate::scenario::handle_at(&pin).is_none(), "the eye hides every pin");
}

#[test]
fn a_pin_behind_the_history_marker_is_hidden_not_gone() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "Thicker");
    h.app.run("timeline.rollback", json!({"position": 0})).unwrap();
    h.frames(5);
    let m = &listed(&h)[0];
    assert_eq!(m.found.state, "hidden", "{:?}", m.found);
    assert_eq!(m.look(), Look::Hidden);
    assert!(m.place().is_some(), "a hidden pin stands where it was");
}

#[test]
fn with_review_off_m_is_move_and_with_no_file_it_marks_nothing() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10})).unwrap();
    h.frames(5);
    assert!(!take_key(&mut h.app, "M"), "Review is off: M stays Move/Copy's");
    h.app.run("view.review", json!({"visible": true})).unwrap();
    h.frames(3);
    let status = h.app.status.clone();
    assert!(!take_key(&mut h.app, "M"), "an unsaved design has no set to mark in");
    h.frames(3);
    assert!(h.app.review.draft.is_none());
    assert_eq!(h.app.status, status, "nothing is said each frame about the missing file");
    assert!(h.app.review.error.is_none());
    h.app.run("view.review", json!({})).unwrap();
    assert!(!h.app.review.open, "view.review with nothing toggles");
}

#[test]
fn a_screenshot_is_cut_to_the_view_and_attached() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "Look here");
    let id = listed(&h)[0].id;
    crate::test_support::click(&mut h, "review:screenshot");
    let shot = h.app.review.shot.expect("a frame was asked for");
    assert_eq!(shot.markup, id);
    assert!(h.app.shots.queued.iter().any(|(t, ..)| *t == shot.token));
    h.frame();
    assert!(h.app.review.drawn.card.is_none() && h.app.review.drawn.list.is_none(), "the card and list are out of the picture");
    // The frame comes back (headless: one made here) and goes to its markup.
    let image = Arc::new(ColorImage::filled([1600, 1000], Color32::from_rgb(40, 50, 60)));
    receive(&mut h.app, shot.token, image);
    h.frames(3);
    assert!(h.app.review.shot.is_none());
    let m = &listed(&h)[0];
    assert_eq!(m.images.len(), 1, "{m:?}");
    assert!(m.images[0].in_design);
    // The bytes are the design's entry, and the file they came through is gone.
    let bytes = h.app.session.doc.markup_images[&m.images[0].file].as_bytes().to_vec();
    assert_eq!(solidcraft_engine::io::markups::image_kind(&bytes), Some("png"));
    let temp = solidcraft_engine::io::vfs::temp_dir();
    let left = std::fs::read_dir(&temp).into_iter().flatten().flatten().any(|e| {
        e.file_name().to_string_lossy().ends_with(&format!("-{}.png", shot.token))
            && e.file_name().to_string_lossy().starts_with("solidcraft-review-")
    });
    assert!(!left, "the screenshot's passing file is removed");
    // Cut to the view: the window's own bar is left out.
    let view = h.app.viewport.rect.unwrap();
    let picture = image::load_from_memory(&bytes).unwrap();
    assert!(picture.width().abs_diff((view.width() * shot.scale).round() as u32) <= 1, "{view:?} {}", picture.width());
    assert!(picture.height().abs_diff((view.height() * shot.scale).round() as u32) <= 1, "{view:?} {}", picture.height());
}

#[test]
fn the_list_is_not_asked_again_while_nothing_changed() {
    let mut h = reviewed_box();
    let at = top_middle(&h);
    mark(&mut h, at, "One");
    h.frames(2);
    let listed_at = h.app.review.listed_at;
    h.frames(20);
    assert_eq!(h.app.review.listed_at, listed_at, "the set and the model did not change");
}
