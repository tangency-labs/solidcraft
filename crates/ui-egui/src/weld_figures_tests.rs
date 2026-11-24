use super::*;
use egui::{Context, RawInput, Rect};

fn paint(ctx: &Context, tokens: &Tokens, figure: SizeFigure, advanced: bool, width: f32) -> (Rect, Vec<egui::epaint::ClippedShape>) {
    let mut allocated = Rect::NOTHING;
    let mut output = ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600., 500.))), ..Default::default() }, |ui| {
        // Leave room around the allocation so a label cannot pass by being screen-clipped.
        ui.add_space(20.);
        let response = paint_size_figure(ui, tokens, figure, advanced, width);
        assert_eq!(response.sense, Sense::hover());
        allocated = response.rect;
    });
    output.textures_delta.clear();
    (allocated, output.shapes)
}

fn labels(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

fn check_figure(figure: SizeFigure, basic_labels: &[&str]) {
    for tokens in [Tokens::light(), Tokens::dark()] {
        for advanced in [false, true] {
            for width in [164., 352.] {
                for pixels_per_point in [1., 2.] {
                    let ctx = Context::default();
                    ctx.set_pixels_per_point(pixels_per_point);
                    let (rect, first) = paint(&ctx, &tokens, figure, advanced, width);
                    let (second_rect, second) = paint(&ctx, &tokens, figure, advanced, width);
                    assert_eq!(rect, second_rect);
                    assert!((rect.size() - vec2(width, width * 120. / 176.)).length() < 0.51);
                    assert!(!first.is_empty(), "{figure:?}");
                    assert_eq!(first.len(), second.len(), "deterministic {figure:?}, advanced={advanced}");
                    let bounds = rect.expand(4. * width / 176.);
                    for shape in &first {
                        let drawn = shape.shape.visual_bounding_rect();
                        if drawn.is_positive() {
                            assert!(bounds.contains_rect(drawn), "{figure:?}, advanced={advanced}, width={width}: {drawn:?} outside {bounds:?}");
                            assert!(shape.clip_rect.contains_rect(drawn), "the test must observe the whole figure");
                        }
                        if let Shape::Text(text) = &shape.shape {
                            assert!(text.galley.job.sections.iter().all(|section| section.format.italics));
                            assert!(text.galley.job.sections.iter().all(|section| section.format.font_id.family == egui::FontFamily::Monospace));
                        }
                    }
                    let mut expected: Vec<_> = basic_labels.iter().map(|s| (*s).to_owned()).collect();
                    if figure == SizeFigure::Fillet && advanced {
                        expected.push("z1".to_owned());
                    }
                    assert_eq!(labels(&first), expected);
                    // Bounds above inspect shapes; also exercise the actual tessellator.
                    let primitives = ctx.tessellate(first, pixels_per_point);
                    assert!(!primitives.is_empty());
                    for primitive in primitives {
                        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                            assert!(mesh.is_valid());
                            assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn fillet_paints_both_sides_when_advanced() {
    check_figure(SizeFigure::Fillet, &["z", "a"]);
}

#[test]
fn groove_paints_cap_and_two_dimensions() {
    check_figure(SizeFigure::Groove, &["h", "s"]);
}

#[test]
fn spot_paints_sheets_nugget_and_diameter() {
    check_figure(SizeFigure::Spot, &["d"]);
}

#[test]
fn plug_paints_filled_hole_and_fill_dimension() {
    check_figure(SizeFigure::Plug, &["fill"]);
}

#[test]
fn stud_paints_two_collar_welds() {
    check_figure(SizeFigure::Stud, &["collar"]);
}

#[test]
fn overlay_paints_reflected_waves_and_thickness() {
    check_figure(SizeFigure::Overlay, &["t"]);
}

#[test]
fn hatch_segments_stay_inside_concave_plate() {
    // U-shaped plate: the open notch is 10<x<30, y>10. These analytic bounds
    // are independent of the clipping implementation, including its edge-crossing rules.
    let polygon = [pos2(0., 0.), pos2(40., 0.), pos2(40., 40.), pos2(30., 40.), pos2(30., 10.), pos2(10., 10.), pos2(10., 40.), pos2(0., 40.)];
    let segments = hatch_segments(&polygon);
    assert!(!segments.is_empty());
    assert_eq!(segments, hatch_segments(&polygon));
    let mut reversed = polygon;
    reversed.reverse();
    let reverse_segments = hatch_segments(&reversed);
    assert_eq!(segments.len(), reverse_segments.len());
    let mut split_scanline = false;
    for (index, [a, b]) in segments.iter().enumerate() {
        assert!(((b.x - a.x) + (b.y - a.y)).abs() < 1e-4, "45 degree hatch");
        if let Some([next, _]) = segments.get(index + 1) {
            split_scanline |= ((a.x + a.y) - (next.x + next.y)).abs() < 1e-4;
        }
        for step in 0..=64 {
            let p = a.lerp(*b, step as f32 / 64.);
            let eps = 1e-4;
            assert!(p.x >= -eps && p.x <= 40. + eps && p.y >= -eps && p.y <= 40. + eps);
            assert!(!(p.x > 10. + eps && p.x < 30. - eps && p.y > 10. + eps), "hatch crosses notch at {p:?}");
        }
    }
    assert!(split_scanline, "a concave plate must yield disjoint hatch spans");
    assert!(hatch_segments(&[]).is_empty());
}

#[test]
fn dimensions_have_exact_tips_and_outside_tails() {
    for outside in [false, true] {
        let ctx = Context::default();
        let tokens = Tokens::light();
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let drawing = Drawing { painter: ui.painter(), origin: pos2(40., 40.), scale: 1., tokens: &tokens };
            drawing.dimension(pos2(0., 0.), pos2(10., 0.), outside);
        });
        output.textures_delta.clear();
        let triangles: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::Path(path) => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(triangles.len(), 2);
        assert_eq!(triangles[0].points[0], pos2(40., 40.));
        assert_eq!(triangles[1].points[0], pos2(50., 40.));
        let base_x = triangles[0].points[1].x;
        assert!((base_x - if outside { 34.8 } else { 45.2 }).abs() < 1e-4);
        assert!(((triangles[0].points[1].y - 40.).abs() - 1.7).abs() < 1e-4);
        assert!(triangles.iter().all(|p| p.fill == tokens.text));
        let lines: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::LineSegment { points, stroke } => {
                    assert_eq!(stroke.color, tokens.text_dim);
                    Some(*points)
                }
                _ => None,
            })
            .collect();
        if outside {
            assert_eq!(lines, [[pos2(28., 40.), pos2(40., 40.)], [pos2(50., 40.), pos2(62., 40.)]]);
        } else {
            assert_eq!(lines, [[pos2(40., 40.), pos2(50., 40.)]]);
        }
    }
}

#[test]
fn colors_are_read_from_the_supplied_tokens_each_paint() {
    let ctx = Context::default();
    for tokens in [Tokens::light(), Tokens::dark(), Tokens::light()] {
        let (_, shapes) = paint(&ctx, &tokens, SizeFigure::Fillet, false, 176.);
        let fills: Vec<_> = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::Path(path) => Some(path.fill),
                _ => None,
            })
            .collect();
        assert_eq!(fills, [tokens.accent, tokens.text, tokens.text, tokens.text, tokens.text]);
        assert!(shapes.iter().any(|s| matches!(&s.shape, Shape::Circle(c) if c.fill == tokens.icon)));
        assert!(shapes.iter().any(|s| matches!(&s.shape, Shape::LineSegment { stroke, .. } if stroke.color == tokens.text_dim.gamma_multiply(0.75))));
    }
}
