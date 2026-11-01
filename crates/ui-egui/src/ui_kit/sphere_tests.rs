use super::*;

#[test]
fn interactive_preview_keeps_source_usage_and_replace_inside_the_card() {
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for width in [280.0, 305.0, 308.0, 356.0] {
            let ctx = crate::fonts::test_context();
            crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                ui.set_width(width);
                super::super::scope(ui, |ui| {
                    sphere_preview_with_source(
                        ui,
                        "preview",
                        Sphere { color: Tokens::get().render_accent, kind: FinishKind::Metal, opacity: None },
                        "Brushed aluminium",
                        "Used by 4",
                        |ui| super::super::compact_dropdown_menu(ui, "source", "In Design", |_| {}),
                        |ui| super::super::searchable_icon_dropdown(ui, "spin", "Replace…", &mut String::new(), &[]),
                    );
                });
            });
            let card = crate::scenario::rect_of("dialog:list:preview").unwrap();
            assert!(card.width() <= width + 1.0, "{card:?}");
            for child in ["preview", "preview:replace", "preview:chip:0", "preview:chip:1"] {
                assert!(card.contains_rect(crate::scenario::rect_of(child).unwrap()), "{child}");
            }
        }
    }
}

#[test]
fn preview_keeps_sample_chips_and_replace_inside_the_shared_card() {
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for width in [280.0, 356.0] {
            let ctx = crate::fonts::test_context();
            crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                ui.set_width(width);
                super::super::scope(ui, |ui| {
                    sphere_preview(
                        ui,
                        "preview",
                        Sphere { color: Tokens::get().render_accent, kind: FinishKind::Metal, opacity: None },
                        "A long appearance label that must fit the card",
                        ["Library", "Used by 1000"],
                        |ui| super::super::searchable_icon_dropdown(ui, "spin", "Replace…", &mut String::new(), &[]),
                    );
                });
            });
            let card = crate::scenario::rect_of("dialog:list:preview").unwrap();
            let sample = crate::scenario::rect_of("preview").unwrap();
            let replace = crate::scenario::rect_of("preview:replace").unwrap();
            assert!(card.width() <= width + 1.0, "{card:?}");
            assert!(card.contains_rect(sample) && card.contains_rect(replace));
            assert!(replace.left() > sample.right());
            for chip in ["preview:chip:0", "preview:chip:1"] {
                assert!(card.contains_rect(crate::scenario::rect_of(chip).unwrap()));
            }
        }
    }
}

#[test]
fn tiles_keep_three_columns_cell_sphere_badge_and_selection_in_both_themes() {
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        let ctx = crate::fonts::test_context();
        let t = Tokens::get();
        let output = crate::test_support::frame(
            &ctx,
            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))), ..Default::default() },
            |ui| {
                ui.set_width(240.0);
                super::super::scope(ui, |ui| {
                    let tiles = [
                        ("a", "Aluminum - Anodized"),
                        ("b", "Stainless Steel - Polished"),
                        ("c", "Neutral product studio with a very long descriptive label"),
                    ]
                    .map(|(id, name)| SphereTile {
                        id,
                        name,
                        sphere: Sphere { color: t.render_accent, kind: FinishKind::Satin, opacity: None },
                        selected: id == "b",
                        users: usize::from(id == "a"),
                        tip: "Hover details",
                        badge: Some("native"),
                    });
                    assert!(sphere_grid(ui, &tiles).is_none());
                });
            },
        );
        assert!(
            !output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Hover details"))),
            "hover cards must stay closed without a pointer"
        );
        let a = crate::scenario::rect_of("a").unwrap();
        let b = crate::scenario::rect_of("b").unwrap();
        let c = crate::scenario::rect_of("c").unwrap();
        assert_eq!(a.top(), b.top());
        assert_eq!(b.top(), c.top());
        assert!(a.right() < b.left() && b.right() < c.left());
        fn meshes(shape: &egui::Shape) -> usize {
            match shape {
                egui::Shape::Mesh(_) => 1,
                egui::Shape::Vec(shapes) => shapes.iter().map(meshes).sum(),
                _ => 0,
            }
        }
        assert_eq!(output.shapes.iter().map(|shape| meshes(&shape.shape)).sum::<usize>(), 3);
        let accent = crate::theme::TabAccent::current(&ctx);
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Rect(r) if r.stroke.color==accent.ink && r.rect.width()==b.width()))
        );
        for id in ["a", "b", "c"] {
            let tile = crate::scenario::rect_of(id).unwrap();
            let label = crate::scenario::rect_of(&format!("{id}:label")).unwrap();
            let galley = crate::scenario::rect_of(&format!("{id}:galley")).unwrap();
            assert!(tile.contains_rect(label) && label.expand(0.1).contains_rect(galley));
        }
        let long_caption = crate::test_support::text_shapes(&output).find(|text| text.galley.job.text.starts_with("Neutral product")).unwrap();
        assert_eq!(long_caption.galley.rows.len(), 2);
        assert!(long_caption.galley.elided);
        assert_eq!(long_caption.galley.job.wrap.overflow_character, Some('…'));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="native")));
    }
}

#[test]
fn cache_bounds_a_thousand_distinct_inputs_and_evicts_oldest() {
    let t = Tokens::light();
    let mut cache = SphereCache::default();
    let first = Sphere { color: t.render_accent, kind: FinishKind::Satin, opacity: None }.key(32.0);
    let initial = cache.mesh(first, &t);
    assert!(Arc::ptr_eq(&initial, &cache.mesh(first, &t)), "hits reuse shading");
    for index in 0..1000_u16 {
        let key = Sphere { color: Color32::from_rgb(index as u8, (index / 256) as u8, 100), kind: FinishKind::Gloss, opacity: Some(0.8) }
            .key(f32::from(index));
        let mesh = cache.mesh(key, &t);
        assert!(cache.entries.len() <= CACHE_ENTRIES);
        assert!(key.side <= MAX_SIDE);
        assert!(mesh.vertices.len() <= 13 * 49);
        assert!(mesh.vertices.iter().all(|v| v.pos.is_finite() && v.pos.x <= f32::from(MAX_SIDE) && v.pos.y <= f32::from(MAX_SIDE)));
    }
    assert_eq!(cache.entries.len(), CACHE_ENTRIES);
    assert!(!cache.entries.iter().any(|(key, _)| *key == first));
    assert!(!Arc::ptr_eq(&initial, &cache.mesh(first, &t)));
    let mut changed_palette = Tokens::dark();
    changed_palette.on_accent = t.render_accent;
    cache.mesh(first, &changed_palette);
    assert_eq!(cache.entries.len(), 1, "theme changes discard shaded colours");
}

#[test]
fn sphere_inputs_are_finite_bounded_and_finishes_differ() {
    let t = Tokens::light();
    let sphere = Sphere { color: t.render_accent, kind: FinishKind::Glass, opacity: Some(f32::NAN) };
    for size in [f32::NAN, f32::INFINITY, -2.0, 0.0, f32::MAX] {
        let key = sphere.key(size);
        assert!((1..=MAX_SIDE).contains(&key.side));
        assert_eq!(key.opacity, 89);
    }
    let mut previous = None;
    for kind in [FinishKind::Matte, FinishKind::Satin, FinishKind::Gloss, FinishKind::Metal, FinishKind::Glass] {
        let mesh = shade(Sphere { kind, opacity: Some(1.0), ..sphere }.key(32.0), &t);
        let colors: Vec<_> = mesh.vertices.iter().map(|v| v.color).collect();
        if let Some(previous) = previous {
            assert_ne!(colors, previous);
        }
        previous = Some(colors);
    }
}
