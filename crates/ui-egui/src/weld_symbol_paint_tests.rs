use super::*;
use solidcore_record::Vec2;

fn drawing() -> weld::Drawing {
    weld::Drawing {
        convention: weld::Convention::Iso2553_2019A { identification: weld::IdentificationLine::Above },
        items: vec![Item::Stroke {
            path: vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 5.0) }],
            closed: false,
            width_mm: 2.0,
            pattern: None,
        }],
        text: Vec::new(),
        reference_length_mm: 10.0,
    }
}

fn frame(map: impl Fn([f64; 2]) -> Pos2) -> (Rect, Vec<Shape>) {
    let ctx = egui::Context::default();
    let mut bounds = Rect::NOTHING;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        bounds = paint(&drawing(), &map, 2.0, solidcraft_theme::Tokens::get().form_ink, 0.5, ui.painter());
    });
    output.textures_delta.clear();
    (bounds, output.shapes.into_iter().map(|s| s.shape).collect())
}

#[test]
fn weld_symbol_paint_billboard_maps_paper_mm_and_returns_stroke_bounds() {
    let (bounds, shapes) = frame(|[x, y]| pos2(100.0 + 2.0 * x as f32, 200.0 - 2.0 * y as f32));
    let path = shapes.iter().find_map(|s| if let Shape::Path(p) = s { Some(p) } else { None }).expect("stroke");
    assert_eq!(path.points, [pos2(100.0, 200.0), pos2(120.0, 190.0)]);
    assert_eq!(path.stroke.width, 4.0);
    assert_eq!(bounds, path.visual_bounding_rect());
    assert_eq!(bounds, Rect::from_min_max(pos2(98.0, 188.0), pos2(122.0, 202.0)));
    assert!(bounds.contains_rect(Rect::from_min_max(pos2(100.0, 190.0), pos2(120.0, 200.0))));
    println!("billboard ink bounds {bounds:?}");
}

#[test]
fn weld_symbol_paint_rotated_mapping_maps_endpoints_and_bounds() {
    let (bounds, shapes) = frame(|[x, y]| pos2(100.0 + 2.0 * y as f32, 200.0 + 2.0 * x as f32));
    let path = shapes.iter().find_map(|s| if let Shape::Path(p) = s { Some(p) } else { None }).expect("stroke");
    assert_eq!(path.points, [pos2(100.0, 200.0), pos2(110.0, 220.0)]);
    assert_eq!(bounds, path.visual_bounding_rect());
    assert_eq!(bounds, Rect::from_min_max(pos2(98.0, 198.0), pos2(112.0, 222.0)));
    println!("rotated ink bounds {bounds:?}");
}

#[test]
fn weld_symbol_paint_rotates_text_baseline_and_flattens_arcs_for_display() {
    let ctx = egui::Context::default();
    let mut drawing = drawing();
    drawing.items.push(Item::Stroke {
        path: vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 5.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 }],
        closed: false,
        width_mm: 1.0,
        pattern: None,
    });
    drawing.text.push(weld::Text { role: weld::TextRole::Tail, text: "135".into(), baseline: Vec2::new(10.0, 5.0), height_mm: 3.0, width_mm: 8.0 });
    let mut bounds = Rect::NOTHING;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        bounds = paint(
            &drawing,
            |[x, y]| pos2(100.0 + 2.0 * y as f32, 200.0 + 2.0 * x as f32),
            2.0,
            solidcraft_theme::Tokens::get().form_ink,
            1.0,
            ui.painter(),
        );
    });
    output.textures_delta.clear();
    let text = output.shapes.iter().find_map(|s| if let Shape::Text(text) = &s.shape { Some(text) } else { None }).expect("text");
    assert!((text.angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    assert_eq!(text.galley.job.text, "135");
    let paths: Vec<_> = output.shapes.iter().filter_map(|s| if let Shape::Path(path) = &s.shape { Some(path) } else { None }).collect();
    assert_eq!(paths.len(), 2);
    let arc = paths[1];
    assert!(arc.points.len() > 2);
    assert!(arc.points.first().unwrap().distance(pos2(100.0, 210.0)) < 0.001);
    assert!(arc.points.last().unwrap().distance(pos2(110.0, 200.0)) < 0.001);
    assert!(bounds.contains_rect(text.visual_bounding_rect()));
}

#[test]
fn weld_symbol_paint_fit_centres_ink_and_rejects_empty_or_invalid_rects() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let specimen = drawing();
        let rect = Rect::from_min_size(pos2(30.0, 40.0), egui::vec2(120.0, 80.0));
        let fit = fit_into_rect(&specimen, rect, ui.painter()).expect("finite drawing");
        let bounds = paint(&specimen, |point| fit.map(point), fit.pixels_per_mm, solidcraft_theme::Tokens::get().form_ink, 1.0, ui.painter());
        assert!(rect.expand(0.001).contains_rect(bounds));
        assert!(bounds.center().distance(rect.center()) < 0.001);
        assert!((bounds.width() - rect.width()).abs() < 0.001 || (bounds.height() - rect.height()).abs() < 0.001);
        assert!(fit_into_rect(&specimen, Rect::NOTHING, ui.painter()).is_none());
        let empty = weld::Drawing { items: Vec::new(), ..specimen };
        assert!(fit_into_rect(&empty, rect, ui.painter()).is_none());
        assert_eq!(paint(&empty, |_| Pos2::ZERO, f32::NAN, solidcraft_theme::Tokens::get().form_ink, 1.0, ui.painter()), Rect::NOTHING);
        assert_eq!(
            paint(
                &drawing(),
                |[x, y]| pos2(if x > 0.0 { f32::NAN } else { x as f32 }, y as f32),
                1.0,
                solidcraft_theme::Tokens::get().form_ink,
                1.0,
                ui.painter(),
            ),
            Rect::NOTHING
        );
    });
    output.textures_delta.clear();
}
