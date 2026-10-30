use super::*;

fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> Aabb2 {
    Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) }
}

/// The boxes after their moves.
fn apply(boxes: &[Aabb2], moves: &[Vec2]) -> Vec<Aabb2> {
    assert_eq!(boxes.len(), moves.len(), "one move per box");
    boxes.iter().zip(moves).map(|(b, m)| bx(b.min.x + m.x, b.min.y + m.y, b.max.x + m.x, b.max.y + m.y)).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

/// Three notes of different sizes, scattered.
fn notes() -> Vec<Aabb2> {
    vec![bx(10.0, 20.0, 40.0, 26.0), bx(55.0, 70.0, 75.0, 80.0), bx(30.0, 45.0, 32.0, 47.0)]
}

const ALL_LINES: [Line; 6] = [Line::Left, Line::CentreX, Line::Right, Line::Bottom, Line::CentreY, Line::Top];

#[test]
fn align_to_the_selection_uses_its_outer_edge_or_centre() {
    let boxes = notes();
    let union = bx(10.0, 20.0, 75.0, 80.0);
    for line in ALL_LINES {
        let moves = align(&boxes, line, Reference::Selection).unwrap();
        let after = apply(&boxes, &moves);
        let wanted = line.of(&union);
        for (b, m) in after.iter().zip(&moves) {
            assert!(close(line.of(b), wanted), "{line:?}: {b:?} on {wanted}");
            // Only along the line's axis, and the other coordinate exactly 0.
            match line.axis() {
                Axis::X => assert_eq!(m.y, 0.0, "{line:?}"),
                Axis::Y => assert_eq!(m.x, 0.0, "{line:?}"),
            }
        }
    }
    // The outermost box on that side doesn't move at all.
    let left = align(&boxes, Line::Left, Reference::Selection).unwrap();
    assert_eq!(left, vec![Vec2::ZERO, Vec2::new(-45.0, 0.0), Vec2::new(-20.0, 0.0)]);
    let top = align(&boxes, Line::Top, Reference::Selection).unwrap();
    assert_eq!(top, vec![Vec2::new(0.0, 54.0), Vec2::ZERO, Vec2::new(0.0, 33.0)]);
    // Centres of the box around them all: x 42.5, y 50.
    let middle = align(&boxes, Line::CentreY, Reference::Selection).unwrap();
    assert_eq!(middle, vec![Vec2::new(0.0, 27.0), Vec2::new(0.0, -25.0), Vec2::new(0.0, 4.0)]);
    let centre = align(&boxes, Line::CentreX, Reference::Selection).unwrap();
    assert_eq!(centre, vec![Vec2::new(17.5, 0.0), Vec2::new(-22.5, 0.0), Vec2::new(11.5, 0.0)]);
}

#[test]
fn align_to_one_box_keeps_it_still() {
    let boxes = notes();
    for line in ALL_LINES {
        for key in 0..boxes.len() {
            let moves = align(&boxes, line, Reference::Item(key)).unwrap();
            assert_eq!(moves[key], Vec2::ZERO, "{line:?} to box {key}");
            let wanted = line.of(&boxes[key]);
            for b in apply(&boxes, &moves) {
                assert!(close(line.of(&b), wanted), "{line:?} to box {key}");
            }
        }
    }
    let right = align(&boxes, Line::Right, Reference::Item(2)).unwrap();
    assert_eq!(right, vec![Vec2::new(-8.0, 0.0), Vec2::new(-43.0, 0.0), Vec2::ZERO]);
}

#[test]
fn align_to_a_coordinate() {
    let boxes = notes();
    let moves = align(&boxes, Line::Bottom, Reference::At(-12.5)).unwrap();
    assert_eq!(moves, vec![Vec2::new(0.0, -32.5), Vec2::new(0.0, -82.5), Vec2::new(0.0, -57.5)]);
    // The paper's limit itself is a valid line, as long as the boxes stay on the paper.
    let at_limit = align(&[bx(0.0, 0.0, 10.0, 5.0)], Line::Right, Reference::At(MAX_COORDINATE_MM)).unwrap();
    assert_eq!(at_limit, vec![Vec2::new(MAX_COORDINATE_MM - 10.0, 0.0)]);
}

#[test]
fn aligning_twice_moves_nothing() {
    let boxes = notes();
    for line in ALL_LINES {
        for to in [Reference::Selection, Reference::Item(1), Reference::At(33.25)] {
            let once = apply(&boxes, &align(&boxes, line, to).unwrap());
            let again = align(&once, line, to).unwrap();
            assert!(again.iter().all(|m| m.len() <= 1e-12), "{line:?} {to:?}: {again:?}");
        }
    }
    // Already aligned boxes move by exactly 0.
    let lined = vec![bx(5.0, 0.0, 9.0, 1.0), bx(5.0, 3.0, 30.0, 4.0)];
    assert_eq!(align(&lined, Line::Left, Reference::Selection).unwrap(), vec![Vec2::ZERO; 2]);
}

#[test]
fn points_and_lines_are_boxes_too() {
    // A balloon's anchor may be a point; a rule may be a horizontal line.
    let boxes = vec![bx(3.0, 4.0, 3.0, 4.0), bx(10.0, 8.0, 20.0, 8.0)];
    let moves = align(&boxes, Line::CentreX, Reference::Item(0)).unwrap();
    assert_eq!(moves, vec![Vec2::ZERO, Vec2::new(-12.0, 0.0)]);
}

#[test]
fn empty_sets_and_single_boxes() {
    assert_eq!(align(&[], Line::Top, Reference::Selection).unwrap(), Vec::<Vec2>::new());
    assert_eq!(align(&[], Line::Top, Reference::At(3.0)).unwrap(), Vec::<Vec2>::new());
    assert_eq!(distribute(&[], Axis::X, Spread::Gaps).unwrap(), Vec::<Vec2>::new());
    assert_eq!(space(&[], Axis::Y, 2.0).unwrap(), Vec::<Vec2>::new());
    let one = [bx(1.0, 2.0, 3.0, 4.0)];
    assert_eq!(align(&one, Line::Left, Reference::Selection).unwrap(), vec![Vec2::ZERO]);
    assert_eq!(distribute(&one, Axis::Y, Spread::Centres).unwrap(), vec![Vec2::ZERO]);
    assert_eq!(space(&one, Axis::X, 2.0).unwrap(), vec![Vec2::ZERO]);
    // Two boxes are already as evenly spread as they can be.
    let two = [bx(0.0, 0.0, 1.0, 1.0), bx(50.0, 0.0, 70.0, 1.0)];
    for spread in [Spread::Gaps, Spread::Centres] {
        assert_eq!(distribute(&two, Axis::X, spread).unwrap(), vec![Vec2::ZERO; 2]);
    }
}

#[test]
fn distribute_gaps_makes_equal_gaps_between_unequal_boxes() {
    // Widths 10, 30, 5, 20 between x 0 and 100: 35 mm of gaps in 3.
    let boxes = vec![bx(80.0, 0.0, 100.0, 5.0), bx(0.0, 0.0, 10.0, 5.0), bx(70.0, 0.0, 75.0, 5.0), bx(12.0, 1.0, 42.0, 9.0)];
    let moves = distribute(&boxes, Axis::X, Spread::Gaps).unwrap();
    // The outermost two stay exactly; the order is by centre: 1, 3, 2, 0.
    assert_eq!(moves[0], Vec2::ZERO);
    assert_eq!(moves[1], Vec2::ZERO);
    let after = apply(&boxes, &moves);
    let gap = 35.0 / 3.0;
    assert!(close(after[3].min.x, 10.0 + gap), "{:?}", after[3]);
    assert!(close(after[2].min.x, after[3].max.x + gap), "{:?}", after[2]);
    assert!(close(after[0].min.x, after[2].max.x + gap));
    // Only x changes.
    assert!(moves.iter().all(|m| m.y == 0.0));
    for (b, a) in boxes.iter().zip(&after) {
        assert_eq!((b.min.y, b.max.y), (a.min.y, a.max.y));
        assert!(close(b.max.x - b.min.x, a.max.x - a.min.x), "sizes are kept");
    }
}

#[test]
fn distribute_gaps_spreads_an_overlap_evenly_too() {
    // Three 10 mm tall boxes in 20 mm: each overlaps the next by 5 mm afterwards.
    let boxes = vec![bx(0.0, 0.0, 1.0, 10.0), bx(0.0, 3.0, 1.0, 13.0), bx(0.0, 10.0, 1.0, 20.0)];
    let moves = distribute(&boxes, Axis::Y, Spread::Gaps).unwrap();
    assert_eq!(moves, vec![Vec2::ZERO, Vec2::new(0.0, 2.0), Vec2::ZERO]);
}

#[test]
fn distribute_centres_makes_an_even_pitch() {
    let boxes = vec![bx(0.0, 0.0, 2.0, 2.0), bx(0.0, 90.0, 4.0, 100.0), bx(0.0, 10.0, 6.0, 12.0), bx(0.0, 20.0, 2.0, 60.0)];
    let moves = distribute(&boxes, Axis::Y, Spread::Centres).unwrap();
    let after = apply(&boxes, &moves);
    // Centres from 1 to 95: pitch 94 / 3, in the order of the centres (0, 2, 3, 1).
    let pitch = 94.0 / 3.0;
    assert!(close(centre(&after[2], Axis::Y), 1.0 + pitch));
    assert!(close(centre(&after[3], Axis::Y), 1.0 + 2.0 * pitch));
    assert_eq!((moves[0], moves[1]), (Vec2::ZERO, Vec2::ZERO));
    assert!(moves.iter().all(|m| m.x == 0.0));
}

#[test]
fn distributing_twice_moves_nothing() {
    let boxes = vec![bx(3.0, 0.0, 7.0, 1.0), bx(41.0, 0.0, 44.0, 1.0), bx(9.0, 0.0, 30.0, 1.0), bx(90.0, 0.0, 91.0, 1.0), bx(60.0, 0.0, 61.0, 1.0)];
    for spread in [Spread::Gaps, Spread::Centres] {
        let once = apply(&boxes, &distribute(&boxes, Axis::X, spread).unwrap());
        let again = distribute(&once, Axis::X, spread).unwrap();
        assert!(again.iter().all(|m| m.len() <= 1e-12), "{spread:?}: {again:?}");
    }
}

#[test]
fn a_long_row_does_not_drift() {
    // 10,000 boxes: every one still lands within rounding of its even place.
    let boxes: Vec<Aabb2> = (0..MAX_BOXES).map(|i| (i * 7 % MAX_BOXES) as f64 * 0.37).map(|x| bx(x, 0.0, x + 0.1, 1.0)).collect();
    for spread in [Spread::Gaps, Spread::Centres] {
        let after = apply(&boxes, &distribute(&boxes, Axis::X, spread).unwrap());
        let mut lows: Vec<f64> = after.iter().map(|b| b.min.x).collect();
        lows.sort_by(f64::total_cmp);
        let pitch = (lows[lows.len() - 1] - lows[0]) / (lows.len() - 1) as f64;
        for (k, x) in lows.iter().enumerate() {
            assert!((x - (lows[0] + pitch * k as f64)).abs() <= 1e-8, "{spread:?}: box {k} at {x}");
        }
    }
}

#[test]
fn ties_keep_the_given_order() {
    // Same centre: the first given comes first, whatever the sizes.
    let boxes = vec![bx(0.0, 0.0, 1.0, 1.0), bx(-5.0, 0.0, 15.0, 1.0), bx(4.0, 0.0, 6.0, 1.0), bx(20.0, 0.0, 22.0, 1.0)];
    let moves = distribute(&boxes, Axis::X, Spread::Centres).unwrap();
    // Order by centre: 0 (0.5), then 2 and 1 (both 5, given 1 before 2), then 3 (21).
    // Pitch 20.5 / 3: box 1 takes the first slot, box 2 the second.
    let after = apply(&boxes, &moves);
    assert!(close(centre(&after[1], Axis::X), 0.5 + 20.5 / 3.0));
    assert!(close(centre(&after[2], Axis::X), 0.5 + 41.0 / 3.0));
    // -0 and 0 are the same place.
    let zeros = vec![bx(-0.0, 0.0, 1.0, 1.0), bx(0.0, 0.0, 1.0, 1.0)];
    let stacked = space(&zeros, Axis::X, 1.0).unwrap();
    assert_eq!(stacked, vec![Vec2::ZERO, Vec2::new(2.0, 0.0)]);
    let stacked = space(&[zeros[1], zeros[0]], Axis::X, 1.0).unwrap();
    assert_eq!(stacked, vec![Vec2::ZERO, Vec2::new(2.0, 0.0)]);
}

#[test]
fn space_stacks_rightwards_from_the_leftmost() {
    let boxes = vec![bx(50.0, 0.0, 60.0, 3.0), bx(5.0, 7.0, 25.0, 9.0), bx(20.0, 1.0, 22.0, 2.0)];
    let moves = space(&boxes, Axis::X, 2.5).unwrap();
    // 1 stays (left edge 5); 2 starts at 27.5, 0 at 32.
    assert_eq!(moves, vec![Vec2::new(-18.0, 0.0), Vec2::ZERO, Vec2::new(7.5, 0.0)]);
    let again = space(&apply(&boxes, &moves), Axis::X, 2.5).unwrap();
    assert_eq!(again, vec![Vec2::ZERO; 3]);
}

#[test]
fn space_stacks_downwards_from_the_top() {
    // Three notes in a list, read top to bottom, 4 mm apart.
    let boxes = vec![bx(0.0, 10.0, 30.0, 15.0), bx(0.0, 100.0, 30.0, 108.0), bx(2.0, 60.0, 20.0, 62.0)];
    let moves = space(&boxes, Axis::Y, 4.0).unwrap();
    // 1 stays (top 108, bottom 100); 2's top goes to 96, its bottom to 94; 0's top to 90.
    assert_eq!(moves, vec![Vec2::new(0.0, 75.0), Vec2::ZERO, Vec2::new(0.0, 34.0)]);
    let after = apply(&boxes, &moves);
    assert_eq!(after[2].max.y, 96.0);
    assert_eq!(after[0].max.y, 90.0);
}

#[test]
fn space_with_no_gap_butts_boxes_together() {
    let boxes = vec![bx(0.0, 0.0, 4.0, 1.0), bx(10.0, 0.0, 13.0, 1.0)];
    assert_eq!(space(&boxes, Axis::X, 0.0).unwrap(), vec![Vec2::ZERO, Vec2::new(-6.0, 0.0)]);
}

#[test]
fn bad_boxes_are_refused() {
    let bad = |b: Aabb2| {
        let boxes = [bx(0.0, 0.0, 1.0, 1.0), b];
        let errors = [
            align(&boxes, Line::Left, Reference::Selection).unwrap_err(),
            distribute(&boxes, Axis::X, Spread::Gaps).unwrap_err(),
            space(&boxes, Axis::Y, 1.0).unwrap_err(),
        ];
        assert!(errors.iter().all(|e| e == &errors[0]), "{errors:?}");
        errors[0].0
    };
    let off = "a box lies within the paper's coordinates";
    assert_eq!(bad(bx(f64::NAN, 0.0, 1.0, 1.0)), off);
    assert_eq!(bad(bx(0.0, 0.0, f64::INFINITY, 1.0)), off);
    assert_eq!(bad(bx(0.0, f64::NEG_INFINITY, 1.0, 1.0)), off);
    assert_eq!(bad(bx(0.0, 0.0, 1.0, MAX_COORDINATE_MM * 1.000_001)), off);
    assert_eq!(bad(Aabb2::EMPTY), off, "the empty box's infinite corners");
    assert_eq!(bad(bx(2.0, 0.0, 1.0, 1.0)), "a box's low corner is at or below and left of its high corner");
    assert_eq!(bad(bx(0.0, 2.0, 1.0, 1.0)), "a box's low corner is at or below and left of its high corner");
    // At the limit is still on the paper.
    let edge = [bx(-MAX_COORDINATE_MM, -MAX_COORDINATE_MM, MAX_COORDINATE_MM, MAX_COORDINATE_MM)];
    assert_eq!(align(&edge, Line::Top, Reference::Selection).unwrap(), vec![Vec2::ZERO]);
}

#[test]
fn too_many_boxes_are_refused() {
    let many = vec![bx(0.0, 0.0, 1.0, 1.0); MAX_BOXES + 1];
    let message = "at most 10,000 boxes are arranged at once";
    assert_eq!(align(&many, Line::Left, Reference::Selection).unwrap_err().0, message);
    assert_eq!(distribute(&many, Axis::X, Spread::Centres).unwrap_err().0, message);
    assert_eq!(space(&many, Axis::X, 1.0).unwrap_err().0, message);
    assert!(space(&many[..MAX_BOXES], Axis::X, 0.0).is_ok());
}

#[test]
fn bad_references_and_gaps_are_refused() {
    let boxes = notes();
    let not_in_set = Invalid("the box to align to is one of the set");
    assert_eq!(align(&boxes, Line::Left, Reference::Item(3)), Err(not_in_set.clone()));
    assert_eq!(align(&boxes, Line::Left, Reference::Item(usize::MAX)), Err(not_in_set.clone()));
    assert_eq!(align(&[], Line::Left, Reference::Item(0)), Err(not_in_set));
    let off = Invalid("the line to align to lies within the paper's coordinates");
    for at in [f64::NAN, f64::INFINITY, -MAX_COORDINATE_MM * 1.000_001] {
        assert_eq!(align(&boxes, Line::Top, Reference::At(at)), Err(off.clone()), "{at}");
    }
    let gap = Invalid("a gap is 0 to 5000 mm");
    for g in [-0.001, f64::NAN, f64::INFINITY, MAX_SIDE_MM + 0.001] {
        assert_eq!(space(&boxes, Axis::X, g), Err(gap.clone()), "{g}");
    }
    assert!(space(&boxes, Axis::X, MAX_SIDE_MM).is_ok());
}

#[test]
fn moves_off_the_paper_are_refused_not_clamped() {
    let off = Invalid("a box would move outside the paper's coordinates");
    // Right edges on the limit: the wide box's left edge is fine, but aligning a point's left to
    // the far edge pushes the wide one out.
    let boxes = [bx(0.0, 0.0, 10.0, 1.0), bx(MAX_COORDINATE_MM - 1.0, 0.0, MAX_COORDINATE_MM, 1.0)];
    assert_eq!(align(&boxes, Line::Left, Reference::Item(1)), Err(off.clone()));
    // Stacking past the limit: the leftmost box stays, the 15 mm one follows it.
    let m = MAX_COORDINATE_MM;
    let near = [bx(m - 30.0, 0.0, m - 20.0, 1.0), bx(m - 20.5, 0.0, m - 5.5, 1.0)];
    assert_eq!(space(&near, Axis::X, 1.0), Ok(vec![Vec2::ZERO, Vec2::new(1.5, 0.0)]), "fits: ends 4 mm inside the limit");
    assert_eq!(space(&near, Axis::X, 6.0), Err(off.clone()));
    // Downwards: the top box stays 5 mm above the limit, the 8 mm one below it goes past.
    let low = [bx(0.0, -m + 5.0, 1.0, -m + 10.0), bx(0.0, -m, 1.0, -m + 8.0)];
    assert_eq!(space(&low, Axis::Y, 0.0), Err(off));
}

#[test]
fn the_result_does_not_depend_on_the_order_given() {
    // Distinct places: the same boxes in any order end up in the same places.
    let boxes = vec![bx(3.0, 1.0, 8.0, 4.0), bx(40.0, 20.0, 41.0, 30.0), bx(17.0, -6.0, 29.0, -1.0), bx(70.0, 9.0, 72.0, 11.0)];
    let permutations: [[usize; 4]; 4] = [[0, 1, 2, 3], [3, 2, 1, 0], [2, 0, 3, 1], [1, 3, 0, 2]];
    let arrange = |set: &[Aabb2]| -> Vec<Vec<Aabb2>> {
        let runs = [
            align(set, Line::CentreY, Reference::Selection).unwrap(),
            distribute(set, Axis::X, Spread::Gaps).unwrap(),
            distribute(set, Axis::Y, Spread::Centres).unwrap(),
            space(set, Axis::X, 3.0).unwrap(),
            space(set, Axis::Y, 3.0).unwrap(),
        ];
        runs.iter().map(|moves| apply(set, moves)).collect()
    };
    let reference = arrange(&boxes);
    for p in permutations {
        let shuffled: Vec<Aabb2> = p.iter().map(|&i| boxes[i]).collect();
        let results = arrange(&shuffled);
        for (run, result) in results.iter().enumerate() {
            for (k, &i) in p.iter().enumerate() {
                assert_eq!(result[k], reference[run][i], "run {run}, permutation {p:?}");
            }
        }
    }
}

#[test]
fn kinds_have_snake_case_names() {
    let names = |v: serde_json::Value| v.to_string();
    assert_eq!(names(serde_json::to_value(Line::CentreX).unwrap()), "\"centre_x\"");
    assert_eq!(names(serde_json::to_value(Axis::Y).unwrap()), "\"y\"");
    assert_eq!(names(serde_json::to_value(Spread::Gaps).unwrap()), "\"gaps\"");
    assert_eq!(names(serde_json::to_value(Reference::Item(2)).unwrap()), "{\"item\":2}");
    assert_eq!(names(serde_json::to_value(Reference::Selection).unwrap()), "\"selection\"");
    assert_eq!(serde_json::from_str::<Reference>("{\"at\":12.5}").unwrap(), Reference::At(12.5));
}
