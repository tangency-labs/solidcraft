use super::*;

/// A barrel joined under a plate's back edge, its ends flush with the sides of the leaf it hangs
/// from (#827 without its fillets): the join is exact, so the command's reply carries no warning,
/// and the barrel's ends stay in the leaf's sides.
#[test]
fn a_barrel_joined_between_the_sides_of_a_leaf_is_exact() {
    let mut session = Session::default();
    run(&mut session, "solid.box", json!({"length": 40, "width": 20, "height": 6, "corner": [0, 0, 5], "body_name": "Plate"}));
    run(&mut session, "solid.box", json!({"length": 14, "width": 10, "height": 6, "corner": [10, 10, 0], "operation": "join", "targets": ["Plate"]}));
    let reply = run(
        &mut session,
        "solid.cylinder",
        json!({"radius": 5, "height": 14, "axis": "X", "base": [10, 20, 0], "operation": "join", "targets": ["Plate"]}),
    );
    assert!(reply["warning"].is_null(), "{reply}");
    let state = session.model.state();
    let plate = &state.body("Plate").unwrap().body;
    assert_eq!(plate.shell_count(), 1);
    assert!(plate.validity().is_empty(), "{:?}", plate.validity());
    // Every edge ends on a level along x of the plate, the leaf or the barrel.
    for edge in plate.edges(1e-3).unwrap() {
        for p in [edge.points.first(), edge.points.last()].into_iter().flatten() {
            assert!([0., 10., 24., 40.].iter().any(|x| (p.x - x).abs() < 1e-9), "an edge ends at {p:?}");
        }
    }
    // A quarter of the barrel lies in the leaf.
    let expected = 5500. + 0.75 * PI * 25. * 14.;
    let actual = plate.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
    assert!(rel(actual, expected) < 1e-5, "{actual} vs {expected}");
}
