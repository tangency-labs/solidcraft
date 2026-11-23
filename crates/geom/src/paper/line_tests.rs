//! Kinds of line: widths by ISO 128-20, checked against its series.

use super::*;
use crate::paper::{Group, Item, ObjectKind, Scene};
use solidcore_record::{Seg2, Vec2};

const GROUPS: [LineGroup; 7] =
    [LineGroup::Mm0_25, LineGroup::Mm0_35, LineGroup::Mm0_5, LineGroup::Mm0_7, LineGroup::Mm1, LineGroup::Mm1_4, LineGroup::Mm2];
const KINDS: [LineKind; 4] = [LineKind::Visible, LineKind::Hidden, LineKind::Centre, LineKind::Thin];

/// Each group's wide line is its name, and its narrow line two steps down the standard's series;
/// the default group is 0.5 mm, whose wide line today's views draw.
#[test]
fn widths_come_from_the_series_wide_and_narrow() {
    let series = [0.13, 0.18, 0.25, 0.35, 0.5, 0.7, 1.0, 1.4, 2.0];
    for (i, group) in GROUPS.into_iter().enumerate() {
        assert_eq!(LineKind::Visible.width_mm(group), series[i + 2], "{group:?}");
        for narrow in [LineKind::Hidden, LineKind::Centre, LineKind::Thin] {
            assert_eq!(narrow.width_mm(group), series[i], "{narrow:?} {group:?}");
        }
    }
    assert_eq!((LineKind::Visible.width_mm(LineGroup::DEFAULT), LineKind::Thin.width_mm(LineGroup::DEFAULT)), (0.5, 0.25));
}

/// A hidden line is dashed 12 line widths on and 3 off, whatever its group: 3 and 0.75 mm in the
/// default group, 4.2 and 1.05 in the 0.7 mm one. The other kinds stay solid.
#[test]
fn hidden_dashes_are_12_and_3_line_widths_in_any_group() {
    for group in GROUPS {
        let d = LineKind::Hidden.width_mm(group);
        assert_eq!(LineKind::Hidden.dashes_mm(group), Some([12.0 * d, 3.0 * d]), "{group:?}");
        for solid in [LineKind::Visible, LineKind::Centre, LineKind::Thin] {
            assert_eq!(solid.dashes_mm(group), None, "{solid:?} {group:?}");
        }
    }
    assert_eq!(LineKind::Hidden.dashes_mm(LineGroup::DEFAULT), Some([3.0, 0.75]));
    let [dash, gap] = LineKind::Hidden.dashes_mm(LineGroup::Mm0_7).unwrap();
    assert!((dash - 4.2).abs() < 1e-12 && (gap - 1.05).abs() < 1e-12, "{dash} {gap}");
}

/// Every kind in every group is a stroke the scene accepts, on a line across an A0 sheet.
#[test]
fn every_kind_is_a_stroke_the_scene_takes() {
    let path = vec![Seg2::Line { a: Vec2::new(10.0, 10.0), b: Vec2::new(1180.0, 830.0) }];
    for group in GROUPS {
        let items = KINDS.map(|kind| Item::Stroke { path: path.clone(), closed: false, width_mm: kind.width_mm(group), pattern: None });
        let scene = Scene::new(Vec2::new(1189.0, 841.0), vec![Group { object: 1, kind: ObjectKind::View, items: items.to_vec() }]);
        scene.validate().unwrap_or_else(|e| panic!("{group:?}: {e}"));
    }
}

/// A group is saved by its wide line's width as text, and read only by one of the seven names; its
/// default is the 0.5 mm group.
#[test]
fn a_line_group_is_saved_by_its_wide_line() {
    for (group, name) in GROUPS.into_iter().zip(LineGroup::NAMES) {
        assert_eq!(serde_json::to_value(group).unwrap(), serde_json::json!(name));
        assert_eq!(serde_json::from_value::<LineGroup>(serde_json::json!(name)).unwrap(), group);
    }
    for bad in [serde_json::json!("0.6"), serde_json::json!(0.5), serde_json::json!("Mm0_5"), serde_json::json!(""), serde_json::json!(null)] {
        assert!(serde_json::from_value::<LineGroup>(bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(LineGroup::default(), LineGroup::DEFAULT);
}
