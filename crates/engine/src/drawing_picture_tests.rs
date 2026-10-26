use serde_json::{Value, json};
use solidcraft_render::VisualStyle::{Shaded, Visible, VisibleHidden};

use super::*;
use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Each case of the two styles and the hide setting, for a model body and a reference part:
/// the pictures bottom first, as (model body, reference part).
#[test]
fn each_style_and_hide_setting_gives_its_pictures() {
    use Part::{DepthOnly, Left, Painted};
    let both = [false, true];
    let cases = [
        (Visible, VisibleHidden, false, vec![]),
        (Visible, Visible, true, vec![]),
        (Shaded, Shaded, true, vec![(Painted, Painted)]),
        (Shaded, Shaded, false, vec![(Left, Painted), (Painted, Left)]),
        (Shaded, Visible, true, vec![(Painted, DepthOnly)]),
        (Shaded, VisibleHidden, false, vec![(Painted, Left)]),
        (Visible, Shaded, false, vec![(DepthOnly, Painted)]),
        (VisibleHidden, Shaded, true, vec![(DepthOnly, Painted)]),
    ];
    for (display, reference, hides, expected) in cases {
        let got: Vec<(Part, Part)> = pictures(display, reference, hides, &both).into_iter().map(|p| (p[0], p[1])).collect();
        assert_eq!(got, expected, "{display:?} {reference:?} hides {hides}");
    }
    // A picture with nothing painted in it is left out: shaded references, none there.
    assert_eq!(pictures(Visible, Shaded, false, &[false, false]), Vec::<Vec<Part>>::new());
    assert_eq!(pictures(Shaded, Shaded, false, &[false]), [vec![Painted]], "no reference parts: the model's picture alone");
    assert_eq!(pictures(Shaded, Visible, false, &[true]), Vec::<Vec<Part>>::new(), "reference parts alone, lines only");
}

/// A plate placed at the top, and a hinge placed twice holding a pin: with the hinge a reference
/// component, both pins are reference parts and the plate isn't; with the pin itself one, the
/// same; with none, nothing is.
#[test]
fn bodies_placed_through_a_reference_component_are_reference_parts() {
    let reference_flags = |s: &Session| crate::drawing_view::shown(s).unwrap().bodies.iter().map(|b| b.reference).collect::<Vec<bool>>();
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(&mut s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.copy", json!({"component": "Hinge", "translate": [0, 50, 0]}));
    let pin = s.doc.find_component("Pin").unwrap();
    let pins = |s: &Session| s.world_state().bodies.iter().map(|b| b.component == pin).collect::<Vec<bool>>();
    assert_eq!(pins(&s).iter().filter(|p| **p).count(), 2, "{:?}", pins(&s));
    assert_eq!(reference_flags(&s), vec![false; pins(&s).len()]);
    for reference in ["Hinge", "Pin"] {
        run(&mut s, "component.metadata.set", json!({"component": reference, "bom_type": "reference"}));
        assert_eq!(reference_flags(&s), pins(&s), "{reference}");
        run(&mut s, "component.metadata.set", json!({"component": reference, "bom_type": "normal"}));
    }
    run(&mut s, "component.metadata.set", json!({"component": "Plate", "bom_type": "reference"}));
    assert_eq!(reference_flags(&s), pins(&s).iter().map(|p| !p).collect::<Vec<bool>>(), "the plate alone");
}
