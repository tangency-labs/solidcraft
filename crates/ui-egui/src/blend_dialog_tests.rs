//! The Blend Surface dialog (#764): edges at each end, the continuity and bulge of each end and
//! the stitch toggle become `surface.blend`, and Edit Feature brings them back with what the
//! dialog does not show (the seams) kept as they were.
use crate::dialogs::Kind;
use crate::dialogs_surface::Surf;
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::doc::{FeatureKind, kernel::Continuity};
use solidcraft_engine::geom::Vec3;

fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    // An edit is applied off the UI thread: give it the frames it takes.
    for _ in 0..200 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
    }
    assert!(h.app.dialog.is_none(), "{:?} {:?} {:?}", h.app.preview.error, h.app.dialog.as_ref().map(|d| (&d.error, &d.inputs)), h.app.session.log);
}

/// What the blend feature `name` was made with: the bodies at each end, continuity, bulge, seams
/// and stitch.
#[allow(clippy::type_complexity)]
fn blend(h: &Harness, name: &str) -> (String, String, [Continuity; 2], [f64; 2], Option<[Vec3; 2]>, bool) {
    match &h.app.session.doc.find_feature(name).unwrap().kind {
        FeatureKind::SurfaceBlend { from, to, continuity, bulge, seam, stitch, .. } => {
            (from.body.to_string(), to.body.to_string(), *continuity, *bulge, seam.map(|[p, q]| [p.0, q.0]), *stitch)
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn edges_at_each_end_make_a_blend_and_come_back_in_an_edit() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "A"})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 20, 5], "body_name": "B"})).unwrap();
    let from = crate::dialogs::edge_sel(&h.app.session, Vec3::new(20.0, 10.0, 10.0)).unwrap();
    let to = crate::dialogs::edge_sel(&h.app.session, Vec3::new(20.0, 20.0, 15.0)).unwrap();
    h.step(&json!({"call":"ui.start","params":{"command":"surface.blend"}})).unwrap();
    h.frames(3);
    assert!(h.widget("From edges").is_some() && h.widget("Start continuity").is_some() && h.widget("End bulge").is_some());
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = vec![from];
    d.inputs[1].items = vec![to];
    let Kind::Surface(Surf::Blend { continuity, bulge, .. }) = &mut d.kind else { panic!("{:?}", d.kind) };
    *continuity = [Continuity::G2, Continuity::G1];
    *bulge = [0.5, 0.0];
    ok(&mut h);
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    let (a, b, continuity, bulge, seam, stitch) = blend(&h, &name);
    assert_eq!((a.as_str(), b.as_str()), ("A", "B"));
    assert_eq!(continuity, [Continuity::G2, Continuity::G1]);
    assert_eq!((bulge, seam, stitch), ([0.5, 0.0], None, false));
    assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 3, "a new surface body beside the boxes");
    // A blend made by the command with a seam the dialog does not show: an edit keeps it.
    h.app.session.execute("edit.undo", &json!({})).unwrap();
    h.app
        .run(
            "surface.blend",
            json!({"from": {"body": "A", "edges": [[20, 10, 10]]}, "to": {"body": "B", "edges": [[20, 20, 15]]},
                "continuity": ["g2", "g1"], "seam": [[0, 10, 10], [0, 20, 15]]}),
        )
        .unwrap();
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(8);
    let d = h.app.dialog.as_mut().unwrap();
    assert!(matches!(d.kind, Kind::Surface(Surf::Blend { continuity: [Continuity::G2, Continuity::G1], stitch: false, .. })), "{:?}", d.kind);
    assert_eq!((d.inputs[0].items.len(), d.inputs[1].items.len()), (1, 1), "{:?}", d.inputs);
    let Kind::Surface(Surf::Blend { stitch, .. }) = &mut d.kind else { panic!() };
    *stitch = true;
    ok(&mut h);
    let (_, _, continuity, _, seam, stitch) = blend(&h, &name);
    assert_eq!(continuity, [Continuity::G2, Continuity::G1]);
    assert_eq!(seam, Some([Vec3::new(0.0, 10.0, 10.0), Vec3::new(0.0, 20.0, 15.0)]));
    assert!(stitch);
    assert_eq!(h.app.session.world_state().unwrap().bodies.len(), 1, "stitched onto both boxes");
}
