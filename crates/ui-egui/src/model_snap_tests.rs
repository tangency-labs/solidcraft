//! Hovering the model locks the cursor onto the conventional snap places of whatever is under
//! it: the end and middle of an edge, the centre of a hole or of a planar face, a quarter turn
//! round a circle, and otherwise a point that slides along the edge or over the face. The
//! tolerance is in screen points, so it does not change with the zoom, and the suppress chord
//! turns the whole thing off.

use egui::Pos2;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::geom::snap::{self, SnapKind};

use crate::scenario::Harness;
use crate::viewport::{Hit, Proj};

const BODY: &str = "Body1";

/// A 40 × 30 × 20 block (its corner at the origin) seen from the front-top, so the top face and
/// its edges are all visible and none is edge-on.
fn block() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    h.settle();
    h
}

fn proj_of(h: &Harness) -> Proj {
    crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap())
}

/// The mesh edge running between `a` and `b` (either way round).
fn edge_between(h: &Harness, a: Vec3, b: Vec3) -> usize {
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    m.edges
        .iter()
        .enumerate()
        .find(|(i, pts)| {
            m.edge_pickable(*i)
                && match (pts.first(), pts.last()) {
                    (Some(p), Some(q)) => (p.dist(a) < 1e-6 && q.dist(b) < 1e-6) || (p.dist(b) < 1e-6 && q.dist(a) < 1e-6),
                    _ => false,
                }
        })
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("no edge between {a:?} and {b:?}"))
}

/// The mesh edge lying on a circle of radius `r` about `centre` (a hole has one at either end).
fn circular_edge(h: &Harness, r: f64, centre: Vec3) -> usize {
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    m.edges
        .iter()
        .enumerate()
        .find(|(i, pts)| m.edge_pickable(*i) && snap::circle_of(pts).is_some_and(|c| (c.radius - r).abs() < 1e-3 && c.center.dist(centre) < 1e-6))
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("no circular edge of radius {r} about {centre:?}"))
}

fn edge_hit(h: &Harness, index: usize) -> Hit {
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    let mid = snap::polyline_mid(m.edges.get(index).unwrap());
    Hit::Edge { body: BODY.into(), index, mid, point: mid }
}

/// What the cursor locks onto `px` screen points away from the world point `at`.
fn snap_near(h: &Harness, hits: &[Hit], at: Vec3, px: egui::Vec2) -> Option<crate::model_snap::Snap> {
    let proj = proj_of(h);
    let pos = proj.to_screen(at).unwrap() + px;
    crate::model_snap::find(&h.app, &proj, Some(pos), hits)
}

fn clear() {
    crate::drag_snap::set_suppressed(false);
    crate::model_snap::set_hold(false);
    crate::model_snap::clear();
}

#[test]
fn hovering_near_the_end_of_an_edge_locks_onto_the_corner() {
    clear();
    let h = block();
    let (a, b) = (Vec3::new(0.0, 0.0, 20.0), Vec3::new(40.0, 0.0, 20.0));
    let hits = [edge_hit(&h, edge_between(&h, a, b))];
    let s = snap_near(&h, &hits, a, egui::vec2(3.0, 2.0)).unwrap_or_else(|| panic!("a corner in reach"));
    assert_eq!(s.kind, SnapKind::Vertex, "{s:?}");
    assert!(s.at.dist(a) < 1e-9, "{s:?}");
    assert_eq!(s.label(), "End");
    // Far from either end it is the middle that is in reach, not the corner.
    let mid = snap_near(&h, &hits, (a + b) * 0.5, egui::vec2(2.0, 1.0)).unwrap_or_else(|| panic!("a middle in reach"));
    assert_eq!(mid.kind, SnapKind::Midpoint, "{mid:?}");
    assert!(mid.at.dist((a + b) * 0.5) < 1e-9, "{mid:?}");
    assert_eq!(mid.label(), "Mid");
}

#[test]
fn hovering_between_the_places_of_an_edge_slides_along_it() {
    clear();
    let h = block();
    let (a, b) = (Vec3::new(0.0, 0.0, 20.0), Vec3::new(40.0, 0.0, 20.0));
    let hits = [edge_hit(&h, edge_between(&h, a, b))];
    // A quarter of the way along is far from the ends and from the middle.
    let quarter = a.lerp(b, 0.25);
    let s = snap_near(&h, &hits, quarter, egui::vec2(0.0, 0.0)).unwrap_or_else(|| panic!("a place along the edge"));
    assert_eq!(s.kind, SnapKind::OnEdge, "{s:?}");
    assert!(s.at.dist(quarter) < 0.2, "it slides to where the cursor is: {s:?}");
    assert!(!s.exact(), "a place that slides is the fallback: {s:?}");
}

#[test]
fn the_rim_of_a_hole_offers_its_centre_and_its_quarter_turns() {
    clear();
    let mut h = block();
    h.app.run("solid.cylinder", json!({"radius": 6, "height": 40, "base": [20, 15, -10], "operation": "cut", "targets": [BODY]})).unwrap();
    h.settle();
    let centre = Vec3::new(20.0, 15.0, 20.0);
    let rim = circular_edge(&h, 6.0, centre);
    let hits = [edge_hit(&h, rim)];
    // Hovering the rim offers the centre, however far across the screen it is.
    let on_rim = Vec3::new(26.0, 15.0, 20.0);
    snap_near(&h, &hits, on_rim, egui::vec2(0.0, 0.0));
    let offered = crate::model_snap::offered();
    assert!(offered.iter().any(|s| s.kind == SnapKind::Center && s.at.dist(centre) < 1e-6), "{offered:?}");
    assert!(offered.iter().any(|s| s.kind == SnapKind::Quadrant), "{offered:?}");
    // Taking it is a matter of putting the cursor on it.
    let s = snap_near(&h, &hits, centre, egui::vec2(2.0, 1.0)).unwrap_or_else(|| panic!("the centre in reach"));
    assert_eq!(s.kind, SnapKind::Center, "{s:?}");
    assert!(s.at.dist(centre) < 1e-6, "{s:?}");
    assert_eq!(s.label(), "Ctr");
    assert_eq!(s.circle.map(|c| (c.radius * 1e3).round() / 1e3), Some(6.0), "{s:?}");
}

/// Moving the cursor off the rim and in towards the centre must not take the centre away with
/// it: once the rim is no longer under the cursor it is the face that has to keep offering it,
/// or the place could be seen and never reached.
#[test]
fn the_centre_of_a_hole_stays_on_offer_from_inside_the_hole() {
    clear();
    let mut h = block();
    h.app.run("solid.cylinder", json!({"radius": 6, "height": 40, "base": [20, 15, -10], "operation": "cut", "targets": [BODY]})).unwrap();
    h.settle();
    let centre = Vec3::new(20.0, 15.0, 20.0);
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    // The wall of the hole, as the pick reports it for a cursor inside the opening.
    let rim = circular_edge(&h, 6.0, centre);
    let wall =
        usize::try_from(*m.edge_faces.get(rim).and_then(|fs| fs.iter().find(|f| snap::planar_face_center(&m, **f).is_none())).unwrap()).unwrap();
    let hits = [Hit::Face { body: BODY.into(), index: wall, point: Vec3::new(17.0, 18.0, 16.0) }];
    let s = snap_near(&h, &hits, centre, egui::vec2(1.0, 1.0)).unwrap_or_else(|| panic!("the centre from inside the hole"));
    assert_eq!(s.kind, SnapKind::Center, "{s:?}");
    assert!(s.at.dist(centre) < 1e-6, "{s:?}");
    // The wall is not planar, so it has no centre of its own to confuse this with.
    assert_eq!(snap::planar_face_center(&m, u32::try_from(wall).unwrap()), None);
}

#[test]
fn hovering_a_planar_face_offers_its_centre() {
    clear();
    let h = block();
    let centre = Vec3::new(20.0, 15.0, 20.0);
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    let face =
        usize::try_from(*m.tri_face.iter().find(|f| snap::planar_face_center(&m, **f).is_some_and(|(c, _)| c.dist(centre) < 1e-6)).unwrap()).unwrap();
    let hits = [Hit::Face { body: BODY.into(), index: face, point: centre }];
    let s = snap_near(&h, &hits, centre, egui::vec2(2.0, 2.0)).unwrap_or_else(|| panic!("a face centre in reach"));
    assert_eq!(s.kind, SnapKind::FaceCenter, "{s:?}");
    assert!(s.at.dist(centre) < 1e-6, "{s:?}");
    assert_eq!(s.label(), "Face");
    // A face place takes it, since the centre of a face lies on the face.
    assert_eq!(s.place_on, Some((BODY.to_string(), face)), "{s:?}");
}

#[test]
fn a_place_away_from_everything_on_a_face_just_slides_over_it() {
    clear();
    let h = block();
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    let centre = Vec3::new(20.0, 15.0, 20.0);
    let face =
        usize::try_from(*m.tri_face.iter().find(|f| snap::planar_face_center(&m, **f).is_some_and(|(c, _)| c.dist(centre) < 1e-6)).unwrap()).unwrap();
    let off = Vec3::new(8.0, 7.0, 20.0);
    let hits = [Hit::Face { body: BODY.into(), index: face, point: off }];
    let s = snap_near(&h, &hits, off, egui::vec2(0.0, 0.0)).unwrap_or_else(|| panic!("a place on the face"));
    assert_eq!(s.kind, SnapKind::OnFace, "{s:?}");
    assert_eq!(s.label(), "", "a place that slides over a face wears no label");
    assert!(s.at.dist(off) < 1e-9, "it stays where the cursor is: {s:?}");
    // The places of the face and of its boundary are all on offer as glyphs; none is in reach,
    // so none is taken.
    let offered = crate::model_snap::offered();
    assert!(offered.iter().any(|o| o.kind == SnapKind::FaceCenter && o.at.dist(centre) < 1e-6), "{offered:?}");
    assert!(offered.iter().any(|o| o.kind == SnapKind::Vertex && o.at.dist(Vec3::new(0.0, 0.0, 20.0)) < 1e-6), "a corner: {offered:?}");
    assert!(offered.iter().any(|o| o.kind == SnapKind::Midpoint), "the middle of a boundary edge: {offered:?}");
    assert!(offered.iter().all(|o| o.exact()), "only exact places are drawn: {offered:?}");
    let proj = proj_of(&h);
    let cursor = proj.to_screen(off).unwrap();
    assert!(offered.iter().all(|o| o.screen.distance(cursor) > 10.0), "none of them is in reach: {offered:?}");
}

#[test]
fn the_reach_is_in_screen_points_so_the_zoom_does_not_change_it() {
    clear();
    let h = block();
    let (a, b) = (Vec3::new(0.0, 0.0, 20.0), Vec3::new(40.0, 0.0, 20.0));
    let hits = [edge_hit(&h, edge_between(&h, a, b))];
    let mut zoomed = block();
    zoomed.app.cam.distance = h.app.cam.distance * 4.0;
    zoomed.settle();
    for (name, h) in [("near", &h), ("far", &zoomed)] {
        // Six points from the corner is in reach at either zoom; twenty is not.
        let took = snap_near(h, &hits, a, egui::vec2(6.0, 0.0));
        assert_eq!(took.as_ref().map(|s| s.kind), Some(SnapKind::Vertex), "{name}: {took:?}");
        let missed = snap_near(h, &hits, a, egui::vec2(20.0, 0.0));
        assert_ne!(missed.as_ref().map(|s| s.kind), Some(SnapKind::Vertex), "{name}: {missed:?}");
    }
}

#[test]
fn a_kind_turned_off_in_the_preferences_is_neither_offered_nor_taken() {
    clear();
    let mut h = block();
    h.app.run("solid.cylinder", json!({"radius": 6, "height": 40, "base": [20, 15, -10], "operation": "cut", "targets": [BODY]})).unwrap();
    h.settle();
    let centre = Vec3::new(20.0, 15.0, 20.0);
    let hits = [edge_hit(&h, circular_edge(&h, 6.0, centre))];
    // On by default.
    let on = snap_near(&h, &hits, centre, egui::vec2(1.0, 1.0)).unwrap_or_else(|| panic!("the centre"));
    assert_eq!(on.kind, SnapKind::Center, "{on:?}");
    assert!(crate::model_snap::offered().iter().any(|s| s.kind == SnapKind::Center));
    // Turned off, the centre is gone from both the glyphs and what the cursor takes; the rest
    // of the rim still snaps.
    h.app.preferences.snap_kinds_off.insert("center".into());
    let off = snap_near(&h, &hits, centre, egui::vec2(1.0, 1.0));
    assert_ne!(off.as_ref().map(|s| s.kind), Some(SnapKind::Center), "{off:?}");
    assert!(crate::model_snap::offered().iter().all(|s| s.kind != SnapKind::Center), "{:?}", crate::model_snap::offered());
    assert!(crate::model_snap::offered().iter().any(|s| s.kind == SnapKind::Quadrant), "a quarter turn is still on");
    // Every exact kind off: the cursor only slides along the geometry.
    for (kind, _) in crate::prefs::SNAP_KIND_SETTINGS {
        h.app.preferences.snap_kinds_off.insert(kind.id().into());
    }
    let sliding = snap_near(&h, &hits, centre, egui::vec2(1.0, 1.0));
    assert_eq!(sliding.as_ref().map(|s| s.kind), Some(SnapKind::OnEdge), "{sliding:?}");
    assert!(crate::model_snap::offered().is_empty(), "no glyphs are left");
    h.app.preferences.snap_kinds_off.clear();
    clear();
}

#[test]
fn the_suppress_chord_turns_snapping_off() {
    clear();
    let h = block();
    let a = Vec3::new(0.0, 0.0, 20.0);
    let hits = [edge_hit(&h, edge_between(&h, a, Vec3::new(40.0, 0.0, 20.0)))];
    assert!(snap_near(&h, &hits, a, egui::vec2(2.0, 0.0)).is_some());
    crate::drag_snap::set_suppressed(true);
    assert_eq!(snap_near(&h, &hits, a, egui::vec2(2.0, 0.0)), None, "the chord holds it off");
    assert!(crate::model_snap::current().is_none());
    assert!(crate::model_snap::offered().is_empty());
    clear();
}

#[test]
fn a_face_place_moves_to_the_snapped_point_and_an_identity_never_does() {
    clear();
    let h = block();
    let st = h.app.session.world_state().unwrap();
    let m = st.body(BODY).unwrap().mesh();
    let centre = Vec3::new(20.0, 15.0, 20.0);
    let face =
        usize::try_from(*m.tri_face.iter().find(|f| snap::planar_face_center(&m, **f).is_some_and(|(c, _)| c.dist(centre) < 1e-6)).unwrap()).unwrap();
    let clicked = Vec3::new(21.0, 16.0, 20.0);
    let edge = edge_between(&h, Vec3::new(0.0, 0.0, 20.0), Vec3::new(40.0, 0.0, 20.0));
    let hits = vec![
        Hit::Face { body: BODY.into(), index: face, point: clicked },
        edge_hit(&h, edge),
        Hit::Vertex { body: BODY.into(), point: Vec3::new(0.0, 0.0, 20.0) },
    ];
    let snapped = snap_near(&h, &hits, centre, egui::vec2(1.0, 1.0)).unwrap_or_else(|| panic!("the face centre"));
    let after = crate::model_snap::apply(&hits, Some(&snapped));
    // The snapped point is the face centre this mesh works out, which is the centre to the last
    // bit or two, not a number typed here.
    let moved = match after.first() {
        Some(Hit::Face { body, index, point }) => (body.clone(), *index, *point),
        other => panic!("the face place: {other:?}"),
    };
    assert_eq!((moved.0.as_str(), moved.1), (BODY, face));
    assert!(moved.2.dist(centre) < 1e-9, "the face place is the snapped point: {:?}", moved.2);
    assert_eq!(after.get(1), hits.get(1), "an edge keeps its own mid, which is how it is found again");
    assert_eq!(after.get(2), hits.get(2), "a vertex keeps its own point");
    // With nothing snapped, or only a place that slides, the hits are untouched.
    assert_eq!(crate::model_snap::apply(&hits, None), hits);
    let sliding = crate::model_snap::Snap { kind: SnapKind::OnFace, ..snapped };
    assert_eq!(crate::model_snap::apply(&hits, Some(&sliding)), hits);
}

/// The Single hole places its centre on the points its own sketch shows and nowhere else, so
/// model snapping stands aside while that input is picking rather than pulling the place to a
/// corner the sketch does not show.
///
/// This gate is load-bearing and not a gap: placing a hole has its own snapping in
/// [`crate::hole_place`] -- the middle of the face and the centres of its round edges, within
/// its own reach, marked and named, Ctrl or Alt to go without, behind Create Hole ‹ Snap to
/// face and edge centres. Letting this one in alongside it takes the pick off those places and
/// onto whatever corner or rim point is nearest instead (a pick on a bore's rim lands on the rim
/// rather than the bore's centre), so the two never run together.
#[test]
fn an_input_that_snaps_to_its_own_sketch_points_keeps_the_cursor() {
    clear();
    let mut h = block();
    let a = Vec3::new(0.0, 0.0, 20.0);
    let hits = [edge_hit(&h, edge_between(&h, a, Vec3::new(40.0, 0.0, 20.0)))];
    assert!(snap_near(&h, &hits, a, egui::vec2(2.0, 0.0)).is_some(), "it snaps with no dialog picking");
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [5, 5], "radius": 2})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.hole");
    h.settle();
    let picking = h.app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| crate::dialogs::snaps_to_points(i.accept));
    assert!(picking, "the Single hole is asking for a face place");
    assert_eq!(snap_near(&h, &hits, a, egui::vec2(2.0, 0.0)), None, "the dialog's own snap owns the pick");
    assert!(crate::model_snap::offered().is_empty(), "and no glyph promises otherwise");
}

/// The origin belongs to no body, so no hit names it: it is offered wherever it is shown.
#[test]
fn the_origin_snaps_although_no_body_owns_it() {
    clear();
    let mut h = block();
    h.app.set_origin_shown(true);
    h.settle();
    let proj = proj_of(&h);
    let pos = proj.to_screen(Vec3::ZERO).unwrap() + egui::vec2(3.0, 2.0);
    let s = crate::model_snap::find(&h.app, &proj, Some(pos), &[]).unwrap_or_else(|| panic!("the origin"));
    assert_eq!(s.kind, SnapKind::Origin, "{s:?}");
    assert!(s.at.dist(Vec3::ZERO) < 1e-9, "{s:?}");
    assert_eq!(s.label(), "Org");
    assert!(s.body.is_empty(), "no body owns it: {s:?}");
    // Turned off in the preferences, it is gone like any other kind.
    h.app.preferences.snap_kinds_off.insert("origin".into());
    let off = crate::model_snap::find(&h.app, &proj, Some(pos), &[]);
    assert_ne!(off.as_ref().map(|s| s.kind), Some(SnapKind::Origin), "{off:?}");
    h.app.preferences.snap_kinds_off.clear();
    clear();
}

#[test]
fn nothing_under_the_cursor_snaps_to_nothing() {
    clear();
    let h = block();
    let proj = proj_of(&h);
    assert_eq!(crate::model_snap::find(&h.app, &proj, Some(Pos2::new(5.0, 5.0)), &[]), None);
    assert_eq!(crate::model_snap::find(&h.app, &proj, None, &[edge_hit(&h, 0)]), None, "no cursor, no snap");
    // A hit naming a body that is not there is ignored rather than trusted.
    let gone = [
        Hit::Edge { body: "Nope".into(), index: 0, mid: Vec3::ZERO, point: Vec3::ZERO },
        Hit::Face { body: "Nope".into(), index: 0, point: Vec3::ZERO },
    ];
    assert_eq!(crate::model_snap::find(&h.app, &proj, Some(Pos2::new(5.0, 5.0)), &gone), None);
}

/// A body being dragged sits under the cursor the whole way, so its own corners and edges are
/// kept out of the candidates while it is the one on the move: snapping to itself would pin it
/// where it stands.
#[test]
fn the_body_on_the_move_offers_no_places_to_snap_to() {
    clear();
    let h = block();
    let hits = [
        edge_hit(&h, 0),
        Hit::Face { body: "Body1".into(), index: 0, point: Vec3::ZERO },
        Hit::Vertex { body: "Other".into(), point: Vec3::ZERO },
    ];
    let kept = crate::model_snap::without(&hits, &["Body1".to_string()]);
    assert_eq!(kept, vec![hits[2].clone()], "only what is not on the move is left");
    assert_eq!(crate::model_snap::without(&hits, &[]), hits.to_vec(), "nothing moving, nothing dropped");
    // With every hit on the moving body, the hover has nothing left to lock onto.
    let proj = proj_of(&h);
    let own = crate::model_snap::without(&hits[..2], &["Body1".to_string()]);
    assert!(own.is_empty());
    assert_eq!(crate::model_snap::find(&h.app, &proj, Some(Pos2::new(5.0, 5.0)), &own), None, "no places left to take");
}

/// A snapped drag lands the body it moves **on** the place, where the body then covers it: the
/// next frame has only the mover under the cursor, which offers no places of its own. The place
/// is held there rather than coming and going frame by frame, which would shake the body between
/// the place and the free drag. Over empty space it is let go, so the drag is free again.
#[test]
fn a_place_the_mover_covers_is_held_rather_than_lost() {
    clear();
    let h = block();
    let corner = Vec3::new(0.0, 0.0, 20.0);
    let hits = [edge_hit(&h, edge_between(&h, corner, Vec3::new(40.0, 0.0, 20.0)))];
    let taken = snap_near(&h, &hits, corner, egui::vec2(3.0, 2.0)).unwrap_or_else(|| panic!("a corner in reach"));
    assert!(taken.at.dist(corner) < 1e-9, "{taken:?}");
    crate::model_snap::set_hold(true);
    let held = crate::model_snap::find(&h.app, &proj_of(&h), Some(Pos2::new(5.0, 5.0)), &[]);
    assert_eq!(held.map(|s| s.at), Some(corner), "the place stands while the mover sits on it");
    crate::model_snap::set_hold(false);
    assert_eq!(crate::model_snap::find(&h.app, &proj_of(&h), Some(Pos2::new(5.0, 5.0)), &[]), None, "let go once it is not");
}
