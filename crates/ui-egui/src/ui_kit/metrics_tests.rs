use crate::theme::Tokens;
use crate::ui_kit::metrics::dimension;
use crate::ui_kit::test_support::*;
use crate::ui_kit::*;

use crate::scenario;

#[test]
fn non_finite_and_tiny_row_widths_are_sanitized() {
    assert_eq!(dimension(f32::NAN), 0.0);
    assert_eq!(dimension(f32::INFINITY), 0.0);
    assert_eq!(dimension(-10.0), 0.0);
    let ctx = crate::fonts::test_context();
    for width in [0.0, 1.0, 12.0] {
        frame(&ctx, vec![], |ui| {
            ui.set_width(width);
            row(ui, "Tiny", |ui| {
                let mut value = String::new();
                field(ui, &mut value);
            });
        });
        assert!(scenario::rect_of("dialog:row:Tiny").unwrap().is_finite());
    }
}

#[test]
fn a_growing_form_widens_past_the_cap_and_publishes_the_card_extra() {
    // #10620: inside `grow_form` the label column takes the width the widest label needs,
    // past the 1.6x cap an ordinary form clamps to, and the card's owner is told by how much
    // the card must widen so the control column keeps its room.
    let ctx = egui::Context::default();
    crate::fonts::setup_fonts(&ctx);
    let t = Tokens::get();
    let key = egui::Id::new("grow-test");
    let long = "Anpassung an Leitkurve";
    let mut label = None;
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_width(300.0);
            scope_grow(ui, key, |ui| {
                let mut value = String::new();
                row(ui, long, |ui| field(ui, &mut value));
            });
        });
        output.textures_delta.clear();
        label = scenario::rect_of(&format!("dialog:label:{long}"));
    }
    let label = label.unwrap();
    let measured = ctx.fonts_mut(|f| f.layout_no_wrap(long.to_owned(), egui::FontId::proportional(t.font_label), t.dialog_label)).size().x;
    assert!(label.width() > t.form_label_width * 1.6, "past the ordinary cap: {label:?}");
    assert!(label.width() >= measured, "the label fits its column: {label:?} vs {measured}");
    let extra = ctx.data(|d| d.get_temp::<f32>(key)).unwrap();
    assert!((extra - (label.width() - t.form_label_width * 1.6)).abs() < 0.01, "the card grows only past the cap: {extra}");
}

#[test]
fn file_tiles_are_fixed_height_bounded_and_disabled_actions_stay_inert() {
    let ctx = crate::fonts::test_context();
    for width in [80.0, 264.0, 680.0] {
        frame(&ctx, vec![], |ui| {
            ui.set_width(width);
            let tile = FileView::action_tile(ui, "file:new", width, "new", "New design", "Millimetres", true);
            assert_eq!(tile.rect.height(), crate::theme::Tokens::get().file_tile_height);
            assert!(tile.rect.width() <= width);
            let row = FileView::nav_row(ui, "file:save", "Save", "Ctrl+S", false, false);
            assert!(!row.enabled());
            assert!(!row.clicked());
            assert_eq!(row.rect.height(), crate::theme::Tokens::get().file_nav_height);
        });
    }
}

#[test]
fn file_grid_has_only_between_row_gaps_and_recovery_actions_fit_narrow_widths() {
    let ctx = crate::fonts::test_context();
    frame(&ctx, vec![], |ui| {
        ui.set_width(280.0);
        let mut rects = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        let before = ui.cursor().top();
        FileView::grid(ui, "grid", 240.0, 2, |ui, _, width| {
            rects.push(FileView::action_tile(ui, "tile", width, "new", "New", "Millimetres", true).rect);
        });
        assert!((rects[1].top() - rects[0].bottom() - crate::theme::Tokens::get().file_grid_gap).abs() < 0.1);
        assert!(
            (ui.cursor().top() - before - 2.0 * crate::theme::Tokens::get().file_tile_height - crate::theme::Tokens::get().file_grid_gap).abs() < 0.1
        );
        let (recover, discard) = FileView::recovery_banner(
            ui,
            "recovery",
            crate::ui_kit::file_view::RecoveryMessage {
                title: "Recovered",
                before_filename: " ",
                filename: "enclosure.solidcraft",
                after_filename: " · just now",
            },
            "Recover",
            Some("Discard"),
        );
        let discard = discard.expect("single-entry banner has discard");
        assert!(!recover.rect.intersects(discard.rect));
        assert!(recover.rect.is_positive() && discard.rect.is_positive());
        assert!(recover.rect.left() >= ui.min_rect().left());
        assert!(discard.rect.right() <= ui.max_rect().right());
    });
}

#[test]
fn file_grid_cells_use_vertical_layout_even_inside_a_horizontal_parent() {
    let ctx = crate::fonts::test_context();
    frame(&ctx, vec![], |ui| {
        ui.set_width(680.0);
        let mut rects = Vec::new();
        FileView::grid(ui, "aligned", 240.0, 2, |ui, index, width| {
            rects.push(FileView::action_tile(ui, &format!("tile-{index}"), width, "new", "New", "Millimetres", true).rect);
        });
        assert!((rects[0].top() - rects[1].top()).abs() < 0.1);
        let (_, discard) = FileView::recovery_banner(
            ui,
            "summary",
            crate::ui_kit::file_view::RecoveryMessage { title: "2 designs recovered", before_filename: "", filename: "", after_filename: "" },
            "Review",
            None,
        );
        assert!(discard.is_none(), "a summary cannot discard an arbitrary design");
    });
}

#[test]
fn sample_grid_stretches_each_row_without_clipping_long_descriptions() {
    let ctx = crate::fonts::test_context();
    let mut cards = Vec::new();
    let output = ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(680.0, 600.0))), ..Default::default() },
        |ui| {
            ui.set_width(680.0);
            cards = FileView::sample_grid(ui, "samples", &[
                SampleCard { title: "Plate", description: "Short description", icon: "new", thumbnail: None },
                SampleCard { title: "Enclosure", description: "Long descriptive sample text that wraps across several lines while preserving every word through the final marker.", icon: "new", thumbnail: None },
            ]);
        },
    );
    assert_eq!(cards.len(), 2);
    assert!((cards[0].rect.top() - cards[1].rect.top()).abs() < 0.1);
    assert!((cards[0].rect.bottom() - cards[1].rect.bottom()).abs() < 0.1);
    let long = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.contains("final marker") => Some((shape.clip_rect, text)),
            _ => None,
        })
        .expect("the entire description is painted");
    let text_rect = egui::Rect::from_min_size(long.1.pos, long.1.galley.size());
    assert!(cards[1].rect.contains_rect(text_rect));
    assert!(long.0.contains_rect(text_rect), "the complete description must fit its clip");
}
