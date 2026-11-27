use super::*;
use egui::{Context, RawInput, Rect};

fn paint(ctx: &Context, tokens: &Tokens, figure: SizeFigure, advanced: bool, width: f32) -> (Rect, Vec<egui::epaint::ClippedShape>) {
    crate::i18n::install();
    let mut allocated = Rect::NOTHING;
    let mut output = ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600., 500.))), ..Default::default() }, |ui| {
        // Leave room around the allocation so a label cannot pass by being screen-clipped.
        ui.add_space(20.);
        let response = paint_size_figure(
            ui,
            tokens,
            figure,
            &FigureState::from_panel(&crate::weld_panel::WeldPanelState { advanced_open: advanced, partial_penetration: true, ..Default::default() }),
            width,
        );
        assert_eq!(response.response.sense, Sense::hover());
        allocated = response.response.rect;
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
                    if figure == SizeFigure::Overlay && advanced {
                        expected.push("w".into());
                    }
                    if figure == SizeFigure::Seam && advanced {
                        expected.push("l".into());
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
fn fillet_paints_leg_and_throat_dimensions() {
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
    check_figure(SizeFigure::Plug, &["h"]);
}

#[test]
fn stud_paints_diameter_and_two_welds() {
    check_figure(SizeFigure::Stud, &["d"]);
}

#[test]
fn overlay_paints_bead_profile_and_thickness() {
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
            let drawing = Drawing {
                painter: ui.painter(),
                origin: pos2(40., 40.),
                scale: 1.,
                tokens: &tokens,
                figure: SizeFigure::Fillet,
                letters: Default::default(),
                state: &FigureState::from_panel(&crate::weld_panel::WeldPanelState::default()),
            };
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
        // A 6.5-long, 4.4-wide head: base is 40 +/- 6.5 and half-width is 2.2.
        assert!((base_x - if outside { 33.5 } else { 46.5 }).abs() < 1e-4);
        assert!(((triangles[0].points[1].y - 40.).abs() - 2.2).abs() < 1e-4);
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
        // Four filled arrowheads; the curved weld face is a separate mesh fan.
        assert_eq!(fills, [tokens.text, tokens.text, tokens.text, tokens.text]);
        assert!(shapes.iter().any(|s| matches!(&s.shape, Shape::Mesh(mesh) if mesh.vertices.iter().all(|v| v.color == tokens.accent))));
        assert!(shapes.iter().any(|s| matches!(&s.shape, Shape::Circle(c) if c.fill == tokens.icon)));
        assert!(shapes.iter().any(|s| matches!(&s.shape, Shape::LineSegment { stroke, .. } if stroke.color == tokens.text_dim.gamma_multiply(0.75))));
    }
}

#[test]
fn edge_seam_and_tack_have_distinct_drawings_and_dimension_letters() {
    let ctx = Context::default();
    let t = Tokens::light();
    let mut signatures = Vec::new();
    for (figure, basic) in [(SizeFigure::Edge, vec!["w"]), (SizeFigure::Seam, vec!["w"]), (SizeFigure::Tack, vec!["l", "e"])] {
        check_figure(figure, &basic);
        let (_, shapes) = paint(&ctx, &t, figure, false, 176.);
        assert_eq!(labels(&shapes), basic);
        signatures.push(format!("{shapes:?}"));
    }
    assert!(signatures.iter().enumerate().all(|(i, s)| signatures.iter().skip(i + 1).all(|other| other != s)));
}

#[test]
fn figure_state_redraws_contour_blend_side_sizes_penetration_and_tack_mode() {
    use crate::weld_panel::{Face, WeldPanelState};
    crate::i18n::install();
    let ctx = Context::default();
    let tokens = Tokens::light();
    let draw = |figure, panel: &WeldPanelState| {
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            paint_size_figure(ui, &tokens, figure, &FigureState::from_panel(panel), 176.);
        });
        output.textures_delta.clear();
        for shape in &output.shapes {
            assert!(shape.shape.visual_bounding_rect().is_finite());
        }
        format!("{:?}", output.shapes)
    };
    let baseline = WeldPanelState::default();
    let first = draw(SizeFigure::Fillet, &baseline);
    let mut variants = Vec::new();
    let mut p = baseline.clone();
    p.face = Face::Flat;
    variants.push(p);
    let mut p = baseline.clone();
    p.blend_toes = true;
    variants.push(p);
    let mut p = baseline.clone();
    p.side = 1;
    variants.push(p);
    let mut p = baseline.clone();
    p.both_sides = true;
    variants.push(p);
    let mut p = baseline.clone();
    p.size.a = "7".into();
    variants.push(p);
    let mut p = baseline.clone();
    p.size.s = "5".into();
    variants.push(p);
    let mut p = baseline.clone();
    p.advanced_open = true;
    p.size.z1 = "4".into();
    p.size.z2 = "6".into();
    variants.push(p);
    for p in variants {
        assert_ne!(first, draw(SizeFigure::Fillet, &p));
    }
    let mut p = baseline.clone();
    let full = draw(SizeFigure::Groove, &p);
    p.size.h = "5".into();
    assert_ne!(full, draw(SizeFigure::Groove, &p));
    p.partial_penetration = true;
    p.size.s = "3".into();
    assert_ne!(full, draw(SizeFigure::Groove, &p));
    for root in [1, 2] {
        p.root = root;
        assert_ne!(full, draw(SizeFigure::Groove, &p));
    }
    let mut grooves = Vec::new();
    for groove in 0..9 {
        p.groove = groove;
        grooves.push(draw(SizeFigure::Groove, &p));
    }
    assert!(grooves.iter().enumerate().all(|(i, s)| grooves.iter().skip(i + 1).all(|other| other != s)));
    let mut modes = Vec::new();
    for mode in 0..3 {
        p.tack_mode = mode;
        modes.push(draw(SizeFigure::Tack, &p));
    }
    assert!(modes.iter().enumerate().all(|(i, s)| modes.iter().skip(i + 1).all(|other| other != s)));
}

#[test]
fn advanced_dimensions_change_their_drawn_geometry() {
    use crate::weld_panel::WeldPanelState;
    crate::i18n::install();
    let geometry = |figure, panel: &WeldPanelState| {
        let ctx = Context::default();
        let mut output = ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600., 500.))), ..Default::default() }, |ui| {
            paint_size_figure(ui, &Tokens::light(), figure, &FigureState::from_panel(panel), 176.);
        });
        output.textures_delta.clear();
        let shapes: Vec<_> = output.shapes.into_iter().filter(|s| !matches!(&s.shape, Shape::Text(_))).collect();
        assert!(shapes.iter().all(|s| s.shape.visual_bounding_rect().is_finite()));
        for shape in &shapes {
            if let Shape::Mesh(mesh) = &shape.shape {
                assert!(mesh.is_valid());
                assert!(mesh.vertices.iter().all(|vertex| vertex.pos.is_finite()));
            }
        }
        format!("{shapes:?}")
    };
    let mut panel = WeldPanelState { advanced_open: true, ..Default::default() };
    panel.size.e = "20".into();
    panel.size.l = "5".into();
    panel.overlap = "1".into();
    for figure in [SizeFigure::Plug, SizeFigure::Stud, SizeFigure::Tack] {
        let before = geometry(figure, &panel);
        let mut changed = panel.clone();
        changed.size.e = "80".into();
        assert_ne!(before, geometry(figure, &changed), "{figure:?} spacing must change geometry, not only a label");
    }
    let before = geometry(SizeFigure::Tack, &panel);
    let mut changed = panel.clone();
    changed.both_sides = true;
    assert_ne!(before, geometry(SizeFigure::Tack, &changed));
    let mut counted = panel.clone();
    counted.tack_mode = 1;
    let before = geometry(SizeFigure::Tack, &counted);
    counted.size.n = "5".into();
    assert_ne!(before, geometry(SizeFigure::Tack, &counted));
    let before = geometry(SizeFigure::Seam, &panel);
    let mut changed = panel.clone();
    changed.size.l = "25".into();
    assert_ne!(before, geometry(SizeFigure::Seam, &changed));
    let before = geometry(SizeFigure::Overlay, &panel);
    let mut changed = panel.clone();
    changed.size.w = "12".into();
    assert_ne!(before, geometry(SizeFigure::Overlay, &changed));
    let mut changed = panel.clone();
    changed.overlap = "4".into();
    assert_ne!(before, geometry(SizeFigure::Overlay, &changed));
    panel.spans = true;
    let before = geometry(SizeFigure::Fillet, &panel);
    let mut changed = panel.clone();
    changed.span_pitch = "80".into();
    assert_ne!(before, geometry(SizeFigure::Fillet, &changed));
    let mut changed = panel.clone();
    changed.span_length = "20".into();
    assert_ne!(before, geometry(SizeFigure::Fillet, &changed));
    let mut changed = panel.clone();
    changed.staggered = true;
    assert_ne!(before, geometry(SizeFigure::Fillet, &changed));
    panel.spans = false;
    for figure in [SizeFigure::Fillet, SizeFigure::Groove] {
        let mut changed = panel.clone();
        changed.root_gap = "0".into();
        let closed = geometry(figure, &changed);
        changed.root_gap = "3".into();
        assert_ne!(closed, geometry(figure, &changed));
    }
    panel.face = crate::weld_panel::Face::Concave;
    panel.root_gap = "0".into();
    for groove in 0..9 {
        panel.groove = groove;
        let _ = geometry(SizeFigure::Groove, &panel);
    }
}

#[test]
fn span_fields_do_not_replace_tack_size_fields_in_the_sketch() {
    let mut panel =
        crate::weld_panel::WeldPanelState { weld_type: crate::weld_panel::WeldType::Tack, tack_mode: 1, spans: true, ..Default::default() };
    panel.size.n = "7".into();
    panel.size.l = "5".into();
    panel.size.e = "20".into();
    panel.span_count = "3".into();
    panel.span_length = "10".into();
    panel.span_pitch = "40".into();
    let figure = FigureState::from_panel(&panel);
    assert_eq!(figure.values.get("n"), Some(&7.));
    assert_eq!(figure.values.get("l"), Some(&5.));
    assert_eq!(figure.values.get("e"), Some(&20.));
    assert_eq!(figure.span_values, [3., 10., 40.]);
}
