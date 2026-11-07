use super::*;

#[test]
fn oblique_sample_preserves_the_plate_height_and_hole_alignment() {
    let mut p = BreakPreview::oblique_sample();
    assert_eq!(p.bands.len(), 1);
    assert!(p.error.is_none());
    for scale in [0, 1] {
        p.scale = scale;
        assert!(p.refresh());
        let factor = if scale == 0 { 1.0 } else { 0.5 };
        let output = p.output.as_ref().unwrap();
        assert_eq!(output.shortening_axis, [1.0, 0.0]);
        assert!(output.lines.iter().flatten().all(|point| point[1] >= -1e-9 && point[1] <= 70.0 * factor + 1e-9));
        let map = p.map().unwrap();
        for x in [25.0, 215.0] {
            let paper = map.to_paper(Vec2::new(x, 35.0)).unwrap().unwrap();
            assert_eq!(paper.y, 35.0 * factor);
        }
    }
}

#[test]
fn repeated_pairs_pending_cancel_and_history_preserve_completed_breaks() {
    let mut p = BreakPreview { active: true, ..Default::default() };
    p.place(Vec2::new(60.0, 35.0));
    p.place(Vec2::new(120.0, 35.0));
    assert_eq!(p.bands.len(), 1);
    assert!(p.active && p.pending.is_none());
    p.place(Vec2::new(150.0, 35.0));
    p.cancel();
    assert!(p.active && p.pending.is_none());
    assert_eq!(p.bands.len(), 1);
    p.place(Vec2::new(150.0, 35.0));
    p.place(Vec2::new(190.0, 35.0));
    assert_eq!(p.bands.len(), 2);
    p.undo();
    assert_eq!(p.bands.len(), 1);
    p.redo();
    assert_eq!(p.bands.len(), 2);
    p.cancel();
    assert!(!p.active);
}

#[test]
fn invalid_pair_does_not_change_geometry_or_add_history() {
    let mut p = BreakPreview { active: true, ..Default::default() };
    p.place(Vec2::new(60.0, 35.0));
    p.place(Vec2::new(60.0, 35.0));
    assert!(p.error.is_some() && p.bands.is_empty() && p.undo.is_empty());
    p.place(Vec2::new(120.0, 35.0));
    assert_eq!(p.bands.len(), 1);
    let lines = p.output.as_ref().unwrap().lines.clone();
    p.place(Vec2::new(80.0, 35.0));
    p.place(Vec2::new(160.0, 35.0));
    assert_eq!(p.bands.len(), 1);
    assert_eq!(p.output.as_ref().unwrap().lines, lines);
    assert_eq!(p.undo.len(), 1);
}

#[test]
fn history_restore_invalid_at_current_scale_preserves_geometry_and_stacks() {
    let mut p = BreakPreview::default();
    assert!(p.edit(vec![BreakBand { start: 60.0, end: 72.0, gap_mm: 8.0 }]));
    assert!(p.edit(Vec::new()));
    p.scale = 1;
    assert!(p.refresh());
    let lines = p.output.as_ref().unwrap().lines.clone();
    p.undo();
    assert!(p.error.is_some() && p.bands.is_empty());
    assert_eq!((p.undo.len(), p.redo.len()), (2, 0));
    assert_eq!(p.output.as_ref().unwrap().lines, lines);

    p.scale = 0;
    assert!(p.refresh());
    p.undo();
    assert_eq!(p.bands.len(), 1);
    p.undo();
    assert!(p.bands.is_empty());
    p.scale = 1;
    assert!(p.refresh());
    p.redo();
    assert!(p.error.is_some() && p.bands.is_empty());
    assert_eq!((p.undo.len(), p.redo.len()), (0, 2));
    assert_eq!(p.output.as_ref().unwrap().lines, lines);
    p.scale = 0;
    assert!(p.refresh());
    p.redo();
    assert_eq!(p.bands.len(), 1);
}

#[test]
fn removing_the_whole_sample_is_refused_without_losing_the_display() {
    let mut p = BreakPreview { angle: 45.0, ..Default::default() };
    assert!(p.refresh());
    let lines = p.output.as_ref().unwrap().lines.clone();
    let n = p.map().unwrap().normal();
    assert!(!p.edit(vec![BreakBand { start: Vec2::new(240.0, 0.0).dot(n) - 1.0, end: Vec2::new(0.0, 70.0).dot(n) + 1.0, gap_mm: 8.0 }]));
    assert!(p.error.is_some() && p.bands.is_empty() && p.undo.is_empty());
    assert_eq!(p.output.as_ref().unwrap().lines, lines);
}
