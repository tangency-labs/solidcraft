use serde_json::json;
use solidcraft_engine::Sel;

use super::{COMMAND, Pu};
use crate::dialogs::Kind;

fn app() -> crate::SolveApp {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("solid.box", json!({"length": 100, "width": 60, "height": 30, "corner": [0, 0, 0], "body_name": "Block"})).unwrap();
    app
}

fn step(app: &crate::SolveApp) -> Option<Pu> {
    match super::start(app, COMMAND) {
        Some((Kind::PrintUnfold(k), _)) => Some(k),
        _ => None,
    }
}

#[test]
fn the_first_step_opens_the_body_in_a_tab_of_its_own_and_leaves_its_design_alone() {
    let mut app = app();
    assert_eq!((app.docs.count(), step(&app)), (1, Some(Pu::Base)));
    let design = app.session.doc.clone();
    // Step 1 is ready with one flat face, and its Next starts the unfolded print.
    let p = solidcore_record::Vec3::new(50.0, 30.0, 0.0);
    let world = crate::open_meshing::session_world(&app.session);
    let index = solidcraft_engine::doc::appearance::face_index_at(world.body("Block").unwrap(), p).unwrap();
    let mut inputs = super::start(&app, COMMAND).unwrap().1;
    assert!(!super::ready(&app, &Pu::Base, &inputs));
    inputs[0].items = vec![Sel::Face { body: "Block".into(), index, point: p }];
    assert!(super::ready(&app, &Pu::Base, &inputs));
    super::run(&mut app, &Pu::Base, &inputs).unwrap();
    // A second tab shows the copy, at step 2; nothing is ready there until every face hangs on.
    assert_eq!((app.docs.count(), step(&app)), (2, Some(Pu::Edges)));
    assert!(app.session.doc.print_unfold.is_some());
    assert!(!super::ready(&app, &Pu::Edges, &[]));
    let state = super::state(&app).unwrap();
    assert_eq!((state.faces, state.attached, state.bends.len(), state.cuts.len()), (6, 1, 0, 4));
    // Each bend is a command of its own; with all six faces on, Next lays the pattern flat.
    for edge in [[0, -30, 0], [50, 0, 0], [0, 30, 0], [-50, 0, 0], [0, -30, 30]] {
        app.run("print.unfold.bend", json!({"edge": edge})).unwrap();
    }
    let state = super::state(&app).unwrap();
    assert_eq!((state.complete, state.bends.len(), state.cuts.len()), (true, 5, 7));
    assert!(super::ready(&app, &Pu::Edges, &[]));
    super::run(&mut app, &Pu::Edges, &[]).unwrap();
    let flat = step(&app).unwrap();
    assert!(matches!(flat, Pu::Flat { .. }), "{flat:?}");
    // Step 3 waits for a printer; with a bed chosen Next goes to splitting, then to the wall.
    assert!(!super::ready(&app, &flat, &[]));
    app.run("print.unfold.bed", json!({"width": 256, "depth": 256})).unwrap();
    assert!(super::ready(&app, &flat, &[]));
    super::run(&mut app, &flat, &[]).unwrap();
    let split = step(&app).unwrap();
    assert_eq!(split, Pu::Split);
    // Nothing of the box lies on another face, so splitting may be skipped.
    assert!(super::ready(&app, &split, &[]));
    super::run(&mut app, &split, &[]).unwrap();
    let joints = step(&app).unwrap();
    assert!(matches!(joints, Pu::Joints { .. }), "{joints:?}");
    // The film and butt joints need no more than the wall given; a press fit asks for 2.4 mm.
    assert!(super::ready(&app, &joints, &[]));
    app.run("print.unfold.joints", json!({"cuts": "press", "wall": "2 mm"})).unwrap();
    let Pu::Joints { .. } = &joints else { panic!("no joints step") };
    let short = Pu::Joints { wall: "2 mm".into(), bends: 0, cuts: 3, assign: 0 };
    assert!(!super::ready(&app, &short, &[]));
    let enough = Pu::Joints { wall: "2.4 mm".into(), bends: 0, cuts: 3, assign: 0 };
    assert!(super::ready(&app, &enough, &[]));
    // Back to the edges, from any of the steps after.
    app.run("print.unfold.edit", json!({})).unwrap();
    assert_eq!(step(&app), Some(Pu::Edges));
    // The design the body came from is as it was.
    crate::documents::switch(&mut app, 0);
    assert!(app.session.doc.print_unfold.is_none() && *app.session.doc == *design);
    assert_eq!(step(&app), Some(Pu::Base));
}

#[test]
fn a_change_of_the_body_in_its_design_asks_to_discard_the_unfolding_or_undo_the_change() {
    let mut app = app();
    app.run(COMMAND, json!({"body": "Block", "face": [50, 30, 0]})).unwrap();
    app.run("print.unfold.bend", json!({"edge": [0, -30, 0]})).unwrap();
    crate::documents::switch(&mut app, 0);
    // A change that leaves the body as it was asks nothing.
    app.run("parameters.add", json!({"name": "L", "expression": "20 mm"})).unwrap();
    assert_eq!(super::changed_copy(&app), None);
    // A hole through it does. Undo takes the change back and the unfolding stays.
    let hole = json!({"length": 10, "width": 10, "height": 30, "corner": [20, 20, 0], "operation": "cut"});
    app.run("solid.box", hole.clone()).unwrap();
    assert_eq!(super::changed_copy(&app), Some(1));
    super::answer_changed(&mut app, super::Changed::Undo);
    assert_eq!(super::changed_copy(&app), None);
    let bends = |app: &crate::SolveApp| app.docs.slots[1].as_ref().unwrap().session.doc.print_unfold.as_ref().unwrap().bends.len();
    assert_eq!(bends(&app), 1);
    // Changed again and discarded: the other tab starts again from the body with its hole,
    // ten faces now and no bends.
    app.run("solid.box", hole).unwrap();
    assert_eq!(super::changed_copy(&app), Some(1));
    super::answer_changed(&mut app, super::Changed::Discard);
    assert_eq!((super::changed_copy(&app), bends(&app)), (None, 0));
    crate::documents::switch(&mut app, 1);
    assert_eq!(super::state(&app).unwrap().faces, 10);
}
