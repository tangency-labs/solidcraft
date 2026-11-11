use super::*;

fn occurrence(id: u64, component: u64, parent: u64, x: f64) -> Occurrence {
    let mut transform = IDENTITY;
    transform[3][0] = x;
    Occurrence { id, component, parent, name: format!("c{component}:{id}"), transform, grounded: false }
}

fn pose(occurrence: u64, inner: u64, x: f64) -> PlacedPose {
    let mut transform = IDENTITY;
    transform[3][0] = x;
    PlacedPose { occurrence, inner, transform }
}

/// Component 1 placed twice in the design (10 and 11), holding component 2 once (20).
fn two_of_one() -> Document {
    Document { occurrences: vec![occurrence(10, 1, 0, 100.0), occurrence(11, 1, 0, 200.0), occurrence(20, 2, 1, 5.0)], ..Document::default() }
}

#[test]
fn the_inner_occurrences_a_flexible_occurrences_joints_move() {
    // From what stays with the occurrence (`None`), through any number of joints.
    assert_eq!(moving(&[[None, Some(1)], [Some(1), Some(2)]]), BTreeSet::from([1, 2]));
    assert_eq!(moving(&[[Some(2), Some(1)], [Some(1), None]]), BTreeSet::from([1, 2]));
    // Nothing stays: the first joint's first side does, and what is joined to it moves.
    assert_eq!(moving(&[[Some(1), Some(2)], [Some(2), Some(3)]]), BTreeSet::from([2, 3]));
    // Occurrences joined only to each other, beside some that are held, stay where they are.
    assert_eq!(moving(&[[None, Some(1)], [Some(5), Some(6)]]), BTreeSet::from([1]));
    assert!(moving(&[[None, None]]).is_empty());
    assert!(moving(&[[Some(4), Some(4)]]).is_empty());
    assert!(moving(&[]).is_empty());
    // A long chain given from its far end is one walk, not one pass for each of its joints.
    let chain: Vec<[Option<u64>; 2]> = (0..5000u64).rev().map(|k| [Some(k + 1), (k > 0).then_some(k)]).collect();
    assert_eq!(moving(&chain).len(), 5000);
}

#[test]
fn a_pose_is_for_one_occurrence_and_evaluation_does_not_see_it() {
    let mut doc = two_of_one();
    doc.assembly.poses = vec![pose(11, 20, 7.0)];
    let shown = |doc: &Document, path: &[u64]| doc.path_placement(path).map(|(_, m)| m[3][0]);
    assert_eq!((shown(&doc, &[10, 20]), shown(&doc, &[11, 20])), (Some(105.0), Some(207.0)));
    let inner = |placements: Vec<(u64, Vec<u64>, Mat)>| {
        let mut at: Vec<(Vec<u64>, f64)> =
            placements.into_iter().filter(|(_, path, _)| path.len() == 2).map(|(_, path, m)| (path, m[3][0])).collect();
        at.sort_by(|a, b| a.0.cmp(&b.0));
        at
    };
    assert_eq!(inner(doc.placements()), [(vec![10, 20], 105.0), (vec![11, 20], 207.0)]);
    assert_eq!(inner(doc.placements_as_designed()), [(vec![10, 20], 105.0), (vec![11, 20], 205.0)]);
    assert_eq!(doc.posed_worlds().into_iter().map(|(o, i, m)| (o, i, m[3][0])).collect::<Vec<_>>(), [(11, 20, 207.0)]);
    // The first occurrence's copy is what the inner occurrence's own id stands for.
    doc.assembly.poses = vec![pose(10, 20, 9.0)];
    assert_eq!(crate::joints::occurrence_world(&doc, 20)[3][0], 109.0);
    assert_eq!(shown(&doc, &[11, 20]), Some(205.0));
}

#[test]
fn poses_are_kept_in_order_one_for_each_pair() {
    let mut doc = two_of_one();
    doc.assembly.poses = vec![pose(11, 20, 1.0), pose(10, 20, 2.0), pose(11, 20, 3.0)];
    doc.assembly.order_poses();
    assert_eq!(doc.assembly.poses, [pose(10, 20, 2.0), pose(11, 20, 1.0)]);
    // The solver's placement of a copy becomes its pose, in the frame of its occurrence.
    let mut world = IDENTITY;
    world[3][0] = 250.0;
    assert!(doc.apply_placed(&[(11, 20, world)]));
    assert_eq!(doc.assembly.poses, [pose(10, 20, 2.0), pose(11, 20, 50.0)]);
    assert!(!doc.apply_placed(&[(11, 20, world)]), "the same again changes nothing");
    // One that is no placement of an occurrence is not kept.
    world[3][0] = f64::NAN;
    assert!(!doc.apply_placed(&[(11, 20, world)]));
    assert!(!doc.apply_placed(&[(11, 99, IDENTITY), (99, 20, IDENTITY)]), "occurrences the design has not got");
}

#[test]
fn a_side_on_an_inner_occurrence_names_it_through_its_subassembly() {
    let mut doc = two_of_one();
    let tree = LinkTree { occurrences: std::collections::BTreeMap::from([(1, 20)]), ..LinkTree::default() };
    doc.links.insert(1, crate::DesignLink { path: "unit.sc".into(), feature: 0, inherited: Vec::new(), file_sha256: None, tree: Some(tree) });
    let frame = Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) };
    let through = doc.through_subassembly(&JointOrigin::new(20, frame.clone()));
    assert_eq!((through.occurrence, through.inside.clone()), (10, vec![20]), "the first occurrence's");
    // One on the subassembly's occurrence, or outside any, stays as it is.
    assert_eq!(doc.through_subassembly(&JointOrigin::new(11, frame.clone())), JointOrigin::new(11, frame.clone()));
    assert_eq!(doc.through_subassembly(&through), through);
}

/// Component 1 is a linked subassembly placed twice (10 and 11, both grounded). Inside it: a
/// base (20), which its design holds still, an arm (21), and component 4 twice (30 and 31) with
/// a pin (40) in it. Each occurrence is flexible, with the design's slider between base and arm
/// for itself (100 at 3, 101 at 8); occurrence 10 also has one between the base and the pin of
/// the first place of component 4 (102).
fn two_flexible() -> Document {
    let frame = || Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) };
    let side = |top: u64, inside: &[u64]| JointOrigin { occurrence: top, inside: inside.to_vec(), snap: frame() };
    let slider = |id: u64, a: JointOrigin, b: JointOrigin, value: f64| Joint {
        id,
        name: format!("J{id}"),
        kind: super::super::JointKind::Slider,
        a,
        b,
        values: vec![value],
        limits: vec![None],
        flip: true,
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        captured: None,
        constraint: None,
    };
    let mut doc = Document {
        occurrences: vec![
            Occurrence { grounded: true, ..occurrence(10, 1, 0, 100.0) },
            Occurrence { grounded: true, ..occurrence(11, 1, 0, 200.0) },
            occurrence(20, 2, 1, 5.0),
            occurrence(21, 3, 1, 5.0),
            occurrence(30, 4, 1, 0.0),
            occurrence(31, 4, 1, 50.0),
            occurrence(40, 5, 4, 1.0),
        ],
        ..Document::default()
    };
    let tree = LinkTree {
        occurrences: BTreeMap::from([(1, 20), (2, 21), (3, 30), (4, 31), (5, 40)]),
        fixed: BTreeSet::from([20]),
        flexible: BTreeMap::from([(10, BTreeMap::from([(7, 100), (8, 102)])), (11, BTreeMap::from([(7, 101)]))]),
        ..LinkTree::default()
    };
    doc.links.insert(1, crate::DesignLink { path: "unit.sc".into(), feature: 0, inherited: Vec::new(), file_sha256: None, tree: Some(tree) });
    doc.assembly.joints = vec![
        slider(100, side(10, &[20]), side(10, &[21]), 3.0),
        slider(101, side(11, &[20]), side(11, &[21]), 8.0),
        slider(102, side(10, &[20]), side(10, &[30, 40]), 2.0),
    ];
    doc
}

#[test]
fn each_flexible_occurrence_has_its_own_copy_in_the_solve_and_the_design_is_not_copied() {
    let doc = two_flexible();
    let st = ModelState::default();
    let expanded = expand(&doc, &st).expect("flexible occurrences");
    // A side on what the joints move is on a copy, one for each occurrence, with an id above
    // the design's own (40); a side on the base, which the design holds still, stays on the
    // occurrence. Only the joints and motion links are the solver's own.
    let sides: Vec<(u64, usize, u64, usize)> =
        expanded.assembly.joints.iter().map(|j| (j.a.occurrence, j.a.inside.len(), j.b.occurrence, j.b.inside.len())).collect();
    assert_eq!(sides, [(10, 1, 41, 0), (11, 1, 43, 0), (10, 1, 42, 0)]);
    assert!(expanded.assembly.poses.is_empty() && expanded.assembly.studies.is_empty());
    let sol = super::super::solve(&doc, &st);
    assert!(sol.errors.is_empty() && sol.conflicts.is_empty(), "{:?} {:?}", sol.errors, sol.conflicts);
    let round = |v: f64| (v * 1e6).round() / 1e6;
    let placed: Vec<(u64, u64, f64, f64)> = sol.placed.iter().map(|(top, inner, m)| (*top, *inner, round(m[3][0]), round(m[3][2].abs()))).collect();
    assert_eq!(placed, [(10, 21, 105.0, 3.0), (10, 40, 105.0, 2.0), (11, 21, 205.0, 8.0)]);
    assert!(sol.world.keys().all(|id| *id <= 40) && sol.fixed.iter().all(|id| *id <= 40), "no id of a copy is left: {:?}", sol.world.keys());
    // An inner occurrence has the degrees of freedom of what carries it, in the first
    // occurrence: the base those of the grounded occurrence, the arm its copy's one.
    assert_eq!((sol.dof.get(&20), sol.dof.get(&21), sol.dof.get(&40)), (Some(&0), Some(&1), Some(&1)));
    let mut doc = doc;
    assert!(doc.apply_joint_solution(&sol));
    let poses: Vec<(u64, u64, f64, f64)> =
        doc.assembly.poses.iter().map(|p| (p.occurrence, p.inner, round(p.transform[3][0]), round(p.transform[3][2].abs()))).collect();
    assert_eq!(poses, [(10, 21, 5.0, 3.0), (10, 40, 5.0, 2.0), (11, 21, 5.0, 8.0)]);
    assert!(!doc.apply_joint_solution(&super::super::solve(&doc, &st)), "solved again, nothing moves");
}

#[test]
fn a_joint_side_is_on_a_copy_on_what_carries_it_or_on_a_second_place() {
    let doc = two_flexible();
    let frame = || Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) };
    let side = |top: u64, inside: &[u64]| JointOrigin { occurrence: top, inside: inside.to_vec(), snap: frame() };
    let sides = [side(11, &[21]), side(11, &[20]), side(10, &[30, 40]), side(10, &[31, 40]), side(21, &[]), side(10, &[]), side(11, &[30, 40])];
    assert_eq!(
        doc.sides_inside(&sides.iter().collect::<Vec<_>>()),
        [
            Inside::Moved { top: 11, inner: 21 },
            Inside::With { top: 11 },
            Inside::Moved { top: 10, inner: 40 },
            // The pin in the second place of component 4: the joints move the first place's.
            Inside::Elsewhere { top: 10 },
            // Named alone, an inner occurrence is the first occurrence's.
            Inside::Moved { top: 10, inner: 21 },
            Inside::Outside,
            // Occurrence 11 has no joint to the pin: it moves with 11 as one.
            Inside::With { top: 11 },
        ]
    );
    // A joint to a second place is left out of the solve and reported, not solved a step late.
    let mut doc = doc;
    let mut stray = doc.assembly.joints.first().cloned().expect("a joint");
    (stray.id, stray.name, stray.a, stray.b) = (103, "Stray".into(), side(11, &[]), side(10, &[31, 40]));
    doc.assembly.joints.push(stray);
    let sol = super::super::solve(&doc, &ModelState::default());
    assert_eq!(sol.errors.iter().map(|(id, _, why)| (*id, why.contains("second place"))).collect::<Vec<_>>(), [(103, true)]);
    assert_eq!(sol.placed.len(), 3, "the other joints are solved as before");
}

#[test]
fn what_the_solver_moves_for_an_occurrence_inside_a_subassembly() {
    let doc = two_flexible();
    let expanded = expand(&doc, &ModelState::default());
    let stood = |occurrence: u64, path: &[u64]| {
        let (id, carry) = stand_in(&doc, expanded.as_ref(), occurrence, path);
        (id, carry[3][0])
    };
    // The arm: each occurrence's own copy; without a path, the first occurrence's.
    assert_eq!((stood(21, &[10, 21]), stood(21, &[11, 21]), stood(21, &[])), ((41, 0.0), (43, 0.0), (41, 0.0)));
    // The base moves with its occurrence, which carries it from where it is inside (x 5).
    assert_eq!((stood(20, &[11, 20]), stood(20, &[])), ((11, 5.0), (10, 5.0)));
    // The pin: its copy in the first place of component 4; nothing in the second place, and
    // occurrence 11, which has no joint to it, carries it as one (x 0 + 1).
    assert_eq!((stood(40, &[10, 30, 40]), stood(40, &[10, 31, 40]), stood(40, &[11, 30, 40])), ((42, 0.0), (40, 0.0), (11, 1.0)));
    // An occurrence of the subassembly stands for itself.
    assert_eq!(stood(10, &[10]), (10, 0.0));
    // A design with rigid occurrences only is solved as it is: nothing is made for the solve.
    let mut rigid = two_flexible();
    rigid.assembly.joints.clear();
    for link in rigid.links.values_mut() {
        link.tree.as_mut().expect("a tree").flexible.clear();
    }
    assert!(expand(&rigid, &ModelState::default()).is_none());
    assert_eq!(stand_in(&rigid, None, 21, &[11, 21]).0, 11);
}

#[test]
fn the_nearest_pose_above_an_occurrence_is_where_it_is_shown() {
    // A pose the linked design has for a flexible subassembly inside it is kept for the
    // occurrence that stands for that subassembly (30, of component 4), and shows wherever
    // that occurrence does; a pose of the occurrence at the top is for what its own joints move.
    let mut doc = two_flexible();
    doc.assembly.poses = vec![pose(10, 21, 6.0), pose(30, 40, 9.0)];
    doc.assembly.order_poses();
    let shown = |path: &[u64]| doc.path_placement(path).map(|(_, m)| m[3][0]);
    assert_eq!((shown(&[10, 21]), shown(&[11, 21])), (Some(106.0), Some(205.0)));
    assert_eq!((shown(&[10, 30, 40]), shown(&[11, 30, 40]), shown(&[10, 31, 40])), (Some(109.0), Some(209.0), Some(151.0)));
}
