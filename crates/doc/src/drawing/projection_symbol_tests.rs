use super::*;
use solidcraft_geom::Aabb2;
use solidcraft_geom::drawing_sheet::{DrawingBorderProfile, Orientation, PaperSize, Sheet};
use solidcraft_geom::paper::{ObjectKind, Scene};
use solidcraft_geom::title_block::default_block;

/// A stroke's pieces as numbers (a line's ends, an arc's centre, radius, start and sweep), whether
/// it is closed, and its width.
fn pieces(item: &Item) -> (Vec<Vec<f64>>, bool, f64) {
    let piece = |seg: &Seg2| match *seg {
        Seg2::Line { a, b } => vec![a.x, a.y, b.x, b.y],
        Seg2::Arc { center, radius, start, sweep } => vec![center.x, center.y, radius, start, sweep],
        _ => panic!("a symbol is lines and arcs"),
    };
    match item {
        Item::Stroke { path, closed, width_mm, .. } => (path.iter().map(piece).collect(), *closed, *width_mm),
        _ => panic!("a symbol is strokes"),
    }
}

/// `a` and `b` the same pieces, each number within 1e-12 mm.
fn same_pieces(a: &[(Vec<Vec<f64>>, bool, f64)], b: &[(Vec<Vec<f64>>, bool, f64)]) -> bool {
    let numbers = |p: &(Vec<Vec<f64>>, bool, f64)| p.0.iter().flatten().copied().chain([p.2]).collect::<Vec<f64>>();
    a.len() == b.len()
        && a.iter().zip(b).all(|(p, q)| {
            let (x, y) = (numbers(p), numbers(q));
            p.1 == q.1 && p.0.len() == q.0.len() && x.len() == y.len() && x.iter().zip(&y).all(|(m, n)| (m - n).abs() < 1e-12)
        })
}

/// ISO 5456-2's symbols, 5 mm across the cone's small end, worked out by hand: 25 mm wide on the
/// outlines' centres (two 10 mm views and the 5 mm gap between them), so from -12.5 to 12.5. The cone
/// is 10 mm long from its 5 mm end on the left to its 10 mm end in both; the end view, circles 10 and
/// 5 mm across, is on the right in first angle (centre 7.5) and on the left in third (-7.5). The
/// outlines are 0.5 mm wide and closed. First come the centre lines, 0.25 mm chain lines (6, 0.75,
/// 0.25 and 0.75 mm, 7.75 in all) running 1.25 mm past the outlines: the axis from -13.75 to 13.75,
/// and the line across the circles' centre from -6.25 to 6.25. Each has its long dash's middle,
/// 3 mm into the pattern, where they cross: the axis meets it 21.25 mm along in first angle (phase
/// 3 - 21.25 + 3 x 7.75 = 5) and 6.25 mm along in third (4.5), the line across 6.25 mm along (4.5).
#[test]
fn the_projection_symbol_is_the_cone_its_end_view_and_their_centre_lines() {
    use std::f64::consts::PI;
    let cone = |x: f64| {
        (vec![vec![x, -2.5, x + 10.0, -5.0], vec![x + 10.0, -5.0, x + 10.0, 5.0], vec![x + 10.0, 5.0, x, 2.5], vec![x, 2.5, x, -2.5]], true, 0.5)
    };
    let circle = |x: f64, r: f64| (vec![vec![x, 0.0, r, 0.0, PI], vec![x, 0.0, r, PI, PI]], true, 0.5);
    let axis = (vec![vec![-13.75, 0.0, 13.75, 0.0]], false, 0.25);
    let across = |x: f64| (vec![vec![x, -6.25, x, 6.25]], false, 0.25);
    let first = [axis.clone(), across(7.5), cone(-12.5), circle(7.5, 5.0), circle(7.5, 2.5)];
    let third = [axis, across(-7.5), cone(2.5), circle(-7.5, 5.0), circle(-7.5, 2.5)];
    let symbol = |projection, centre| projection_symbol(projection, 5.0, centre);
    let drawn = |projection| symbol(projection, Vec2::ZERO).map(|item| pieces(&item));
    assert!(same_pieces(&drawn(Projection::First), &first), "{:?}", drawn(Projection::First));
    assert!(same_pieces(&drawn(Projection::Third), &third), "{:?}", drawn(Projection::Third));
    // The centre lines' patterns, and none on the outlines.
    let phases = |projection| {
        symbol(projection, Vec2::ZERO).map(|item| match item {
            Item::Stroke { pattern: Some(Dashes { lengths_mm, phase_mm }), .. } => (lengths_mm, Some(phase_mm)),
            _ => (Vec::new(), None),
        })
    };
    let chain = vec![6.0, 0.75, 0.25, 0.75];
    let none = || (Vec::new(), None);
    for (projection, axis_phase) in [(Projection::First, 5.0), (Projection::Third, 4.5)] {
        let [axis, across, rest @ ..] = phases(projection);
        assert_eq!((&axis.0, &across.0), (&chain, &chain), "{projection:?}");
        assert!((axis.1.unwrap() - axis_phase).abs() < 1e-12 && (across.1.unwrap() - 4.5).abs() < 1e-12, "{projection:?}: {axis:?} {across:?}");
        assert_eq!(rest, [none(), none(), none()], "{projection:?}");
    }
    // Anywhere else it is the same symbol moved there, its patterns unchanged.
    let moved = symbol(Projection::First, Vec2::new(100.0, 50.0));
    let expected = first.map(|(mut path, closed, width)| {
        for piece in &mut path {
            let ends = if piece.len() == 4 { 2 } else { 1 };
            for at in 0..ends {
                piece[2 * at] += 100.0;
                piece[2 * at + 1] += 50.0;
            }
        }
        (path, closed, width)
    });
    assert!(same_pieces(&moved.clone().map(|item| pieces(&item)), &expected), "{moved:?}");
    let pattern = |item: &Item| match item {
        Item::Stroke { pattern, .. } => pattern.clone(),
        _ => None,
    };
    assert_eq!(moved.each_ref().map(pattern), symbol(Projection::First, Vec2::ZERO).each_ref().map(pattern));
}

/// A chain line is a long dash of 24 line widths, a gap of 3, a one-width dot and a gap of 3, worked
/// out by hand for 0.25 and 0.5 mm lines, its long dash centred where it is asked: 3 mm along a
/// 0.25 mm line is the first dash's middle (phase 0), the line's start is 3 mm into its first dash,
/// and 100 mm along a 0.5 mm line (12, 1.5, 0.5 and 1.5 mm, 15.5 in all) starts 14.5 mm into its
/// pattern. A middle a hair past the first dash's still starts the pattern at 0, never at its
/// length, which no pattern may reach. Each one is a stroke the scene takes.
#[test]
fn a_chain_line_crosses_on_a_long_dash() {
    let quarter = vec![6.0, 0.75, 0.25, 0.75];
    assert_eq!(chain(0.25, 3.0), Dashes { lengths_mm: quarter.clone(), phase_mm: 0.0 });
    assert_eq!(chain(0.25, 0.0), Dashes { lengths_mm: quarter.clone(), phase_mm: 3.0 });
    let half = chain(0.5, 100.0);
    assert_eq!(half.lengths_mm, [12.0, 1.5, 0.5, 1.5]);
    assert!((half.phase_mm - 14.5).abs() < 1e-12, "{half:?}");
    let past = chain(0.25, f64::from_bits(3.0f64.to_bits() + 1));
    assert_eq!(past, Dashes { lengths_mm: quarter, phase_mm: 0.0 });
    let path = vec![Seg2::Line { a: Vec2::new(10.0, 10.0), b: Vec2::new(110.0, 10.0) }];
    for (width_mm, pattern) in [(0.25, past), (0.5, half), (0.25, chain(0.25, 50.0))] {
        let items = vec![Item::Stroke { path: path.clone(), closed: false, width_mm, pattern: Some(pattern) }];
        Scene::new(Vec2::new(420.0, 297.0), vec![Group { object: 1, kind: ObjectKind::View, items }]).validate().unwrap();
    }
}

/// The built-in block on A3 draws its lines, the same in either projection, and then its one
/// symbol, 3.5 mm, centred in its projection cell's value box, (381.175, 45.175) to (408.65, 54.325),
/// in the drawing's projection: 34 records of lines and 15 of the symbol (2 centre lines of one
/// piece, 3 outlines of 4, 2 and 2), and the scene takes them.
#[test]
fn the_built_in_block_draws_its_symbol_in_the_drawings_projection() {
    let sheet = DrawingBorderProfile::iso().layout(&Sheet::standard(PaperSize::A3, Orientation::Landscape)).unwrap();
    let layout = default_block().layout(&sheet).unwrap();
    let centre = Aabb2 { min: Vec2::new(381.175, 45.175), max: Vec2::new(408.65, 54.325) }.center();
    let lines = layout.paper(7);
    let [first, third] = [Projection::First, Projection::Third].map(|projection| title_block_paper(&layout, 7, projection));
    for (group, projection) in [(first, Projection::First), (third, Projection::Third)] {
        assert_eq!((group.object, group.kind), (7, ObjectKind::TitleBlock));
        assert_eq!(group.items[..lines.items.len()], lines.items[..], "{projection:?}");
        let symbol = projection_symbol(projection, 3.5, centre).map(|item| pieces(&item));
        let drawn: Vec<_> = group.items[lines.items.len()..].iter().map(pieces).collect();
        assert!(same_pieces(&drawn, &symbol), "{projection:?}: {drawn:?}");
        assert_eq!((lines.records(), group.records()), (34, 49), "{projection:?}");
        Scene::new(Vec2::new(420.0, 297.0), vec![group]).validate().unwrap();
    }
}
