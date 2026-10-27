use super::*;

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
