use solidcraft_doc::drawing::SheetComponent;
use solidcraft_doc::drawing::table::PartsList;
use solidcraft_geom::cell::Content;

use super::*;

const SPACE: Aabb2 = Aabb2 { min: Vec2 { x: 10.0, y: 10.0 }, max: Vec2 { x: 410.0, y: 287.0 } };

/// Each way of naming a place gives the corner worked out by hand: a table 100 by 50 in a space from
/// (10, 10) to (410, 287). The point given back is the right end of the first part's heading's outer
/// edge, so a table with its heading at the bottom names its lower right, and one split into a second
/// part on its right names the first part's right end, 110 mm left of the outline's.
#[test]
fn a_tables_place_gives_its_corner() {
    let at_origin = |x0: f64, y0: f64, x1: f64, y1: f64| Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) };
    let (top, bottom) = (at_origin(-100.0, -50.0, 0.0, 0.0), at_origin(-100.0, 0.0, 0.0, 50.0));
    // Two parts of 100 by 50, 10 mm apart, the second on the right: 210 by 50 in all.
    let split_right = at_origin(-100.0, -50.0, 110.0, 0.0);
    let anchor = |corner, offset_mm| Place::Anchor { corner, offset_mm };
    for (place, top_right) in [
        (anchor(Corner::TopRight, [0.0, 0.0]), [410.0, 287.0]),
        (anchor(Corner::TopLeft, [0.0, 0.0]), [110.0, 287.0]),
        (anchor(Corner::BottomRight, [0.0, 0.0]), [410.0, 60.0]),
        (anchor(Corner::BottomLeft, [0.0, 0.0]), [110.0, 60.0]),
        (anchor(Corner::TopRight, [-5.0, -7.0]), [405.0, 280.0]),
        (Place::At([20.0, 200.0]), [120.0, 200.0]),
    ] {
        let [x, y] = top_right;
        assert_eq!(corner_of(place, top, SPACE), Vec2::new(x, y), "{place:?}");
        assert_eq!(corner_of(place, bottom, SPACE), Vec2::new(x, y - 50.0), "{place:?}");
    }
    let anchor = |corner| Place::Anchor { corner, offset_mm: [0.0, 0.0] };
    assert_eq!(corner_of(anchor(Corner::TopLeft), split_right, SPACE), Vec2::new(110.0, 287.0), "the first part at the left");
    assert_eq!(corner_of(anchor(Corner::TopRight), split_right, SPACE), Vec2::new(300.0, 287.0), "the second part at the right");
}

/// Only a column that shows a mass makes a list weigh its parts: a field named like it in other text
/// doesn't, and neither does a list without one.
#[test]
fn only_a_mass_column_weighs_a_list() {
    let design = SheetComponent { source: 1, component: 0 };
    let list = PartsList {
        of: design,
        view: Default::default(),
        group: Default::default(),
        fold: false,
        purchased_contents: false,
        mass: Default::default(),
    };
    let mut table = Kind::PartsList(list).default_table();
    assert!(shows_mass(&table), "the default list shows a mass");
    table.columns.retain(|column| column.name != "mass");
    assert!(!shows_mass(&table));
    if let Some(column) = table.columns.first_mut() {
        column.content = Content(vec![Part::Text("mass".into())]);
    }
    assert!(!shows_mass(&table), "the word alone");
    assert!(!shows_mass(&Kind::Revision.default_table()));
}
