use serde_json::json;
use solidcore_record::Vec3;

use super::*;

/// Every field has its default, and the settings read and write back the same.
#[test]
fn view_settings_default_to_a_front_view_that_fits_its_page() {
    let settings: ViewSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(settings, ViewSettings::default());
    assert_eq!(settings.view, ViewDirection::Named(NamedView::Front));
    assert_eq!((settings.scale, settings.visual_style, settings.tangent_edges), (None, VisualStyle::Visible, TangentEdges::Hide));
    assert_eq!(settings.validate().unwrap(), NamedView::Front.basis());
    assert_eq!(serde_json::to_value(settings).unwrap().get("scale"), None, "no scale is written, not null");
    let full = json!({
        "view": {"direction": [0.0, 1.0, 0.0], "up": [0.0, 0.0, 1.0], "origin": [1.0, 2.0, 3.0]},
        "scale": "1:2", "visual_style": "visible_hidden", "reference_style": "shaded",
        "reference_hides_model": true, "reference_shown_percent": 5.0, "tangent_edges": "show",
    });
    let read: ViewSettings = serde_json::from_value(full.clone()).unwrap();
    assert_eq!(serde_json::to_value(read).unwrap(), full);
    assert_eq!(read.validate().unwrap().origin, Vec3::new(1.0, 2.0, 3.0));
    let iso: ViewSettings = serde_json::from_value(json!({"view": "iso"})).unwrap();
    assert_eq!(iso.view, ViewDirection::Named(NamedView::IsoFrontRight));
}

/// What the settings refuse: unknown fields (`display_style` among them: the field is `visual_style`,
/// #5307), unknown names, a basis that can't be built and a margin out of range.
#[test]
fn view_settings_refuse_what_they_cant_draw() {
    for not_settings in [
        json!({"scael": "1:2"}),
        json!({"view": 3}),
        json!({"view": "Front"}),
        json!({"view": {"direction": [0, 1, 0]}}),
        json!({"view": {"direction": [0, 1, 0], "up": [0, 0, 1], "roll": 1}}),
        json!({"scale": 0.5}),
        json!({"visual_style": "hidden"}),
        json!({"display_style": "visible"}),
        json!({"tangent_edges": "thin"}),
    ] {
        assert!(serde_json::from_value::<ViewSettings>(not_settings.clone()).is_err(), "{not_settings}");
    }
    let refused = [
        json!({"view": {"direction": [0, 0, 0], "up": [0, 0, 1]}}),
        json!({"view": {"direction": [0, 0, 1], "up": [0, 0, -1]}}),
        json!({"reference_shown_percent": -0.001}),
        json!({"reference_shown_percent": 100.001}),
    ];
    for settings in refused {
        let read: ViewSettings = serde_json::from_value(settings.clone()).unwrap();
        assert!(read.validate().is_err(), "{settings}");
    }
    let edge: ViewSettings = serde_json::from_value(json!({"reference_shown_percent": 100.0})).unwrap();
    assert!(edge.validate().is_ok());
    // A saved view's own settings read the same name, and refuse the old one.
    let own: ViewOverrides = serde_json::from_value(json!({"visual_style": "shaded"})).unwrap();
    assert_eq!(own.visual_style, Some(VisualStyle::Shaded));
    assert!(serde_json::from_value::<ViewOverrides>(json!({"display_style": "shaded"})).is_err());
}
