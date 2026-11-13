//! Creative Mode's contextual tools: the features that fit what is selected, for the selection
//! pill ([`crate::creative_mode`]).
//!
//! Each feature on [`FEATURES`] is offered when its own window, opened on the current selection
//! ([`Dialog::for_command`]), takes everything selected and so has its main input filled: two
//! profiles offer Loft, a profile and an edge Sweep, a body Mirror and the patterns, two bodies
//! Combine, and so on. No rule here restates what a window accepts, so a feature is offered
//! exactly where its window would take the selection, and clicking it opens that window filled.
//!
//! Two or more bodies whose boxes overlap also get Join, Cut and Intersect as one-click actions
//! (`solid.combine` on the first selected body with the others as tools).

use std::cell::RefCell;

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::dialogs::Dialog;
use crate::ui_kit::Item;

/// The features that may be offered, most useful first. Each opens its window on the selection.
const FEATURES: &[&str] = &[
    "sketch.create",
    "solid.press_pull",
    "solid.extrude",
    "solid.revolve",
    "solid.loft",
    "solid.sweep",
    "solid.pipe",
    "solid.fillet",
    "solid.chamfer",
    "solid.combine",
    "solid.shell",
    "solid.offset_face",
    "solid.hole",
    "solid.thread",
    "solid.draft",
    "solid.mirror",
    "solid.pattern.rectangular",
    "solid.pattern.circular",
    "solid.pattern.path",
    "solid.align",
    "solid.replace_face",
    "solid.split_body",
    "solid.split_face",
    "solid.scale",
    "solid.emboss",
    "surface.thicken",
    "surface.stitch",
    "construct.plane.midplane",
    "construct.plane.offset",
    "construct.plane.two_edges",
    "construct.plane.three_points",
    "construct.axis.through_cylinder",
    "construct.axis.two_points",
];

/// The ids of the features whose window takes the whole selection, in [`FEATURES`] order.
/// Worked out once per selection and model, since each opens a window to find out.
pub(crate) fn fitting(app: &SolveApp) -> Vec<&'static str> {
    thread_local! {
        static CACHE: RefCell<Option<(Key, Vec<&'static str>)>> = const { RefCell::new(None) };
    }
    let key = Key::of(app);
    if let Some(ids) = CACHE.with_borrow(|c| c.as_ref().filter(|(k, _)| *k == key).map(|(_, ids)| ids.clone())) {
        return ids;
    }
    let ids: Vec<&'static str> = FEATURES.iter().copied().filter(|id| takes_selection(app, id)).collect();
    CACHE.with_borrow_mut(|c| *c = Some((key, ids.clone())));
    ids
}

/// What the cached answer depends on: the selection and the model it is on.
#[derive(Clone, PartialEq)]
struct Key {
    selection: Vec<Sel>,
    features: usize,
    undo: usize,
    redo: usize,
}

impl Key {
    fn of(app: &SolveApp) -> Key {
        Key {
            selection: app.session.selection.clone(),
            features: app.session.doc.features.len(),
            undo: app.session.undo.len(),
            redo: app.session.redo.len(),
        }
    }
}

/// Whether the window of command `id`, opened now, holds every selected item and has its first
/// input filled (a body selected for Extrude only reaches its target bodies, so it does not
/// count). A loft needs two profiles.
fn takes_selection(app: &SolveApp, id: &str) -> bool {
    let sel = &app.session.selection;
    if sel.is_empty() || !solidcraft_engine::find_command(id).is_some_and(|c| c.info(&app.session).enabled) {
        return false;
    }
    // A surface command on a solid body would only fail; on faces and sheets it has its place.
    if id.starts_with("surface.") && sel.iter().any(|x| matches!(x, Sel::Body { .. })) {
        return false;
    }
    // Press Pull has no window: it fits where the context menu offers it.
    let Some(d) = Dialog::for_command(app, id) else {
        return crate::context_menu::items(app, &crate::context_menu::Target::Viewport).iter().any(|i| i.id == id && i.enabled);
    };
    let Some(first) = d.inputs.first() else { return false };
    let least = if id == "solid.loft" { 2 } else { 1 };
    // Combine needs a body to join to and another to join.
    if id == "solid.combine" && sel.len() < 2 {
        return false;
    }
    first.items.len() >= least && sel.iter().all(|x| d.inputs.iter().any(|i| i.items.iter().any(|y| crate::selection::same_item(x, y))))
}

/// The selected bodies, in the order they were selected, when the selection is only bodies.
fn selected_bodies(app: &SolveApp) -> Option<Vec<String>> {
    app.session.selection.iter().map(|s| if let Sel::Body { name } = s { Some(name.clone()) } else { None }).collect()
}

/// Whether the boxes round `a` and each of `others` overlap: Cut and Intersect then have
/// something to act on.
fn overlapping(app: &SolveApp, a: &str, others: &[String]) -> bool {
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ba) = st.body(a).map(|b| b.mesh().bounds()) else { return false };
    others.iter().all(|o| {
        st.body(o).map(|b| b.mesh().bounds()).is_some_and(|bo| {
            ba.min.x < bo.max.x && bo.min.x < ba.max.x && ba.min.y < bo.max.y && bo.min.y < ba.max.y && ba.min.z < bo.max.z && bo.min.z < ba.max.z
        })
    })
}

/// Join, Cut and Intersect for two or more overlapping bodies, applied at once on the first
/// selected body with the others as tools. Empty for any other selection.
pub(crate) fn booleans(app: &SolveApp) -> Vec<Item> {
    let Some(bodies) = selected_bodies(app).filter(|b| b.len() >= 2) else { return Vec::new() };
    let (target, tools) = (&bodies[0], &bodies[1..]);
    if !overlapping(app, target, tools) {
        return Vec::new();
    }
    let names = tools.join(", ");
    [
        ("join", "boolean_union", "creative_mode.join", "creative_mode.join_tip"),
        ("cut", "boolean_subtract", "creative_mode.cut", "creative_mode.cut_tip"),
        ("intersect", "boolean_intersect", "creative_mode.intersect", "creative_mode.intersect_tip"),
    ]
    .into_iter()
    .map(|(operation, icon, label, tip)| {
        let mut item =
            crate::context_menu::cmd(app, "solid.combine", app.tr(label)).with(json!({"target": target, "tools": tools, "operation": operation}));
        // The pill shows icons only: each operation has its own.
        item.icon = icon.into();
        item.tip = Some(app.tr(tip).replace("{target}", target).replace("{tools}", &names));
        item
    })
    .collect()
}

/// A center pivot pole, made at once: a construction axis through the middle of the selection, to
/// revolve, pattern or turn things round. One flat face gets a pole through its centroid, square
/// to it; one body gets an upright pole (along Z) through the centre of its box. `None` for any
/// other selection.
pub(crate) fn pole(app: &SolveApp) -> Option<Item> {
    let (id, params, of) = match app.session.selection.as_slice() {
        [Sel::Face { body, index, point }] => {
            let (centroid, _) = crate::dialogs::planar_face(&app.session, body, *index)?;
            let params = json!({
                "face": {"face": [point.x, point.y, point.z]},
                "point": {"point": [centroid.x, centroid.y, centroid.z]},
                "name": "Center Pole",
            });
            ("construct.axis.normal_to_face", params, body.clone())
        }
        [Sel::Body { name }] => {
            let st = crate::open_meshing::session_world(&app.session);
            let b = st.body(name)?.mesh().bounds();
            let (cx, cy) = ((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
            if b.max.z - b.min.z <= 1e-9 {
                return None;
            }
            let params = json!({
                "a": {"point": [cx, cy, b.min.z]},
                "b": {"point": [cx, cy, b.max.z]},
                "name": "Center Pole",
            });
            ("construct.axis.two_points", params, name.clone())
        }
        _ => return None,
    };
    let mut item = crate::context_menu::cmd(app, id, app.tr("creative_mode.pole")).with(params);
    item.icon = "axis_center_pole".into();
    item.tip = Some(app.tr("creative_mode.pole_tip").replace("{target}", &of));
    Some(item)
}

/// The pill's feature items for the selection: the one-click booleans first, then each fitting
/// feature, which opens its window, with the center pole third among them so a full pill keeps
/// it. Combine's window is left out where the booleans stand in for it.
pub(crate) fn items(app: &SolveApp) -> Vec<Item> {
    let mut v = booleans(app);
    let quick = !v.is_empty();
    let first = v.len();
    for id in fitting(app) {
        if quick && id == "solid.combine" {
            continue;
        }
        v.push(crate::context_menu::cmd(app, id, solidcraft_ui_text::tr_label(crate::command_label(id))));
    }
    if let Some(p) = pole(app) {
        v.insert((first + 2).min(v.len()), p);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    /// Two 20 mm boxes, the second shifted 10 mm in x and y so they overlap, or 40 mm so they
    /// do not.
    fn two_boxes(shift: f64) -> Harness {
        let mut h = Harness::new();
        h.app.preferences.creative_mode = true;
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0]})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [shift, shift, 0], "operation": "new_body"})).unwrap();
        h.settle();
        let m = h.app.session.execute("inspect.measure", &json!({})).unwrap();
        let bodies: Vec<String> = m["bodies"].as_array().unwrap().iter().filter_map(|b| b["name"].as_str().map(str::to_string)).collect();
        assert_eq!(bodies.len(), 2, "{bodies:?}");
        let items: Vec<_> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
        h.app.run("select.set", json!({"items": items})).unwrap();
        h
    }

    fn ids(h: &Harness) -> Vec<String> {
        items(&h.app).into_iter().map(|i| i.id).collect()
    }

    fn labels(h: &Harness) -> Vec<String> {
        items(&h.app).into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn two_overlapping_bodies_offer_join_cut_and_intersect_first() {
        let h = two_boxes(10.0);
        let labels = labels(&h);
        assert_eq!(&labels[..3], ["Join", "Cut", "Intersect"], "{labels:?}");
        let icons: Vec<String> = booleans(&h.app).iter().map(|i| i.icon.to_string()).collect();
        assert!(icons[0] != icons[1] && icons[1] != icons[2], "each has its own icon: {icons:?}");
    }

    #[test]
    fn two_bodies_apart_offer_combine_but_not_cut() {
        let h = two_boxes(40.0);
        assert!(booleans(&h.app).is_empty());
        assert!(ids(&h).iter().any(|i| i == "solid.combine"), "{:?}", ids(&h));
    }

    #[test]
    fn clicking_cut_takes_the_second_body_out_of_the_first() {
        let mut h = two_boxes(10.0);
        let cut = booleans(&h.app).into_iter().nth(1).unwrap();
        crate::context_menu::run_item(&mut h.app, &cut, egui::Pos2::ZERO);
        h.settle();
        let m = h.app.session.execute("inspect.measure", &json!({})).unwrap();
        let bodies = m["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 1, "{m}");
        let volume = bodies[0]["volume_mm3"].as_f64().unwrap();
        assert!((volume - 3000.0).abs() < 1.0, "a 10 x 10 corner is cut from 4000 mm³: {volume}");
    }

    #[test]
    fn one_body_offers_mirror_and_patterns_but_not_extrude() {
        let mut h = two_boxes(40.0);
        h.app.run("select.set", json!({"items": [{"type": "body", "name": "Body1"}]})).unwrap();
        let ids = ids(&h);
        assert!(ids.iter().any(|i| i == "solid.mirror"), "{ids:?}");
        assert!(ids.iter().any(|i| i == "solid.pattern.rectangular"), "{ids:?}");
        assert!(!ids.iter().any(|i| i == "solid.extrude" || i == "solid.combine"), "{ids:?}");
    }

    #[test]
    fn two_flat_faces_offer_a_midplane() {
        let mut h = two_boxes(40.0);
        h.app
            .run(
                "select.set",
                json!({"items": [{"type": "face", "body": "Body1", "point": [10, 10, 10]}, {"type": "face", "body": "Body1", "point": [10, 10, 0]}]}),
            )
            .unwrap();
        let ids = ids(&h);
        assert!(ids.iter().any(|i| i == "construct.plane.midplane"), "{ids:?}");
    }

    #[test]
    fn every_offered_feature_opens_its_window_on_the_selection() {
        let selections = [
            json!([{"type": "body", "name": "Body1"}]),
            json!([{"type": "body", "name": "Body1"}, {"type": "body", "name": "Body2"}]),
            json!([{"type": "face", "body": "Body1", "point": [10, 10, 10]}]),
            json!([{"type": "edge", "body": "Body1", "point": [10, 0, 10]}]),
            json!([{"type": "face", "body": "Body1", "point": [10, 10, 10]}, {"type": "face", "body": "Body1", "point": [10, 10, 0]}]),
        ];
        for items in selections {
            let mut h = two_boxes(40.0);
            h.app.run("select.set", json!({"items": items})).unwrap();
            for id in fitting(&h.app) {
                h.app.dialog = None;
                h.app.start(id);
                assert!(h.app.dialog.is_some(), "{id} opened its window for {items}");
                h.app.dialog = None;
                h.app.run("select.set", json!({"items": items})).unwrap();
            }
        }
    }

    fn click_pole(h: &mut Harness) {
        let p = pole(&h.app).expect("a center pole is offered");
        assert!(items(&h.app).iter().any(|i| i.label == "Center Pole"), "the pill has it");
        crate::context_menu::run_item(&mut h.app, &p, egui::Pos2::ZERO);
        h.settle();
    }

    #[test]
    fn a_body_gets_an_upright_center_pole_and_a_second_one_too() {
        let mut h = two_boxes(40.0);
        h.app.run("select.set", json!({"items": [{"type": "body", "name": "Body1"}]})).unwrap();
        let before = h.app.session.doc.features.len();
        click_pole(&mut h);
        assert_eq!(h.app.session.doc.features.len(), before + 1, "one axis feature");
        h.app.run("select.set", json!({"items": [{"type": "body", "name": "Body2"}]})).unwrap();
        click_pole(&mut h);
        assert_eq!(h.app.session.doc.features.len(), before + 2, "a second pole");
    }

    #[test]
    fn a_flat_face_gets_a_pole_through_its_middle() {
        let mut h = two_boxes(40.0);
        h.app.run("select.set", json!({"items": [{"type": "face", "body": "Body1", "point": [3, 4, 10]}]})).unwrap();
        let before = h.app.session.doc.features.len();
        let p = pole(&h.app).unwrap();
        let params = p.params.clone().unwrap();
        let at = params["point"]["point"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
        assert!((at[0] - 10.0).abs() < 1e-6 && (at[1] - 10.0).abs() < 1e-6 && (at[2] - 10.0).abs() < 1e-6, "the top face's middle: {at:?}");
        click_pole(&mut h);
        assert_eq!(h.app.session.doc.features.len(), before + 1);
    }

    /// Where the pole made last stands: its line's point at z = 0.
    fn pole_foot(h: &Harness) -> (f64, f64) {
        let st = h.app.session.model.state();
        let c = st.construct.last().expect("a pole");
        let solidcraft_engine::doc::construct::ConstructGeom::Axis { origin, dir } = c.geom else { panic!("not an axis") };
        let t = -origin.z / dir.z;
        (origin.x + t * dir.x, origin.y + t * dir.y)
    }

    /// Move Body1 30 mm along X with the Move window, Keep Axes With Body set to `keep`.
    fn move_body1(h: &mut Harness, keep: bool) {
        h.app.run("select.set", json!({"items": [{"type": "body", "name": "Body1"}]})).unwrap();
        h.app.start("solid.move");
        let d = h.app.dialog.as_mut().unwrap();
        if let crate::dialogs::Kind::Move { x, .. } = &mut d.kind {
            *x = "30 mm".into();
        }
        d.extra.insert("keep_axes".into(), json!(keep));
        let d = h.app.dialog.take().unwrap();
        let p = crate::dialogs_move::params(&h.app, &d).unwrap();
        h.app.run("solid.move", p).unwrap();
        h.settle();
    }

    #[test]
    fn moving_a_body_takes_its_center_pole_along_unless_told_not_to() {
        for (keep, foot) in [(true, 40.0), (false, 10.0)] {
            let mut h = two_boxes(40.0);
            h.app.run("select.set", json!({"items": [{"type": "body", "name": "Body1"}]})).unwrap();
            click_pole(&mut h);
            move_body1(&mut h, keep);
            let (x, y) = pole_foot(&h);
            assert!((x - foot).abs() < 1e-6 && (y - 10.0).abs() < 1e-6, "keep {keep}: the pole stands at ({x}, {y})");
        }
    }

    #[test]
    fn a_face_pole_goes_with_its_body_too() {
        let mut h = two_boxes(40.0);
        h.app.run("select.set", json!({"items": [{"type": "face", "body": "Body1", "point": [3, 4, 10]}]})).unwrap();
        click_pole(&mut h);
        move_body1(&mut h, true);
        let (x, _) = pole_foot(&h);
        assert!((x - 40.0).abs() < 1e-6, "the face pole moved: {x}");
    }

    #[test]
    fn two_bodies_or_an_edge_get_no_pole() {
        let mut h = two_boxes(40.0);
        assert!(pole(&h.app).is_none());
        h.app.run("select.set", json!({"items": [{"type": "edge", "body": "Body1", "point": [10, 0, 10]}]})).unwrap();
        assert!(pole(&h.app).is_none());
    }

    /// A finished sketch with one 10 mm circle at the origin, on XY raised by `z`; its id.
    fn circle_sketch(h: &mut Harness, z: f64) -> u64 {
        let id = h.app.run("sketch.create", json!({"plane": "XY", "offset": z})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 5})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.settle();
        id
    }

    #[test]
    fn two_profiles_offer_a_loft() {
        let mut h = Harness::new();
        h.app.preferences.creative_mode = true;
        let (low, high) = (circle_sketch(&mut h, 0.0), circle_sketch(&mut h, 20.0));
        h.app
            .run("select.set", json!({"items": [{"type": "profile", "sketch": low, "index": 0}, {"type": "profile", "sketch": high, "index": 0}]}))
            .unwrap();
        let ids = ids(&h);
        assert!(ids.iter().any(|i| i == "solid.loft"), "{ids:?}");
    }

    #[test]
    fn one_profile_does_not_offer_a_loft() {
        let mut h = Harness::new();
        h.app.preferences.creative_mode = true;
        let low = circle_sketch(&mut h, 0.0);
        h.app.run("select.set", json!({"items": [{"type": "profile", "sketch": low, "index": 0}]})).unwrap();
        let ids = ids(&h);
        assert!(ids.iter().any(|i| i == "solid.extrude"), "{ids:?}");
        assert!(!ids.iter().any(|i| i == "solid.loft"), "{ids:?}");
    }

    #[test]
    fn a_profile_and_an_edge_offer_a_sweep() {
        let mut h = two_boxes(40.0);
        let profile = circle_sketch(&mut h, 0.0);
        h.app
            .run(
                "select.set",
                json!({"items": [{"type": "profile", "sketch": profile, "index": 0}, {"type": "edge", "body": "Body1", "point": [0, 0, 5]}]}),
            )
            .unwrap();
        let ids = ids(&h);
        assert!(ids.iter().any(|i| i == "solid.sweep"), "{ids:?}");
    }
}
