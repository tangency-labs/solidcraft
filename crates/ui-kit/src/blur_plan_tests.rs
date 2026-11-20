use crate::blur_plan::*;
use egui::{Rect, pos2};

const SCREEN: [u32; 2] = [1000, 800];

fn px(x0: u32, y0: u32, x1: u32, y1: u32) -> PxRect {
    PxRect::new(x0, y0, x1, y1).unwrap()
}

fn marker(job: usize, rect: PxRect) -> Marker {
    Marker { job, rect }
}

#[test]
fn rectangles_without_area_do_not_exist() {
    assert!(PxRect::new(3, 3, 3, 9).is_none());
    assert!(PxRect::new(5, 3, 2, 9).is_none());
    assert!(PxRect::new(2, 2, 3, 3).is_some());
}

#[test]
fn padding_clamps_to_the_screen_and_never_underflows() {
    assert_eq!(px(10, 10, 20, 20).padded(30, SCREEN), Some(px(0, 0, 50, 50)));
    assert_eq!(px(990, 790, 1000, 800).padded(30, SCREEN), Some(px(960, 760, 1000, 800)));
    assert_eq!(px(0, 0, 1000, 800).padded(u32::MAX, SCREEN), Some(px(0, 0, 1000, 800)));
    // Entirely off a smaller screen: nothing left.
    assert_eq!(px(900, 700, 950, 750).padded(5, [100, 100]), None);
}

#[test]
fn a_level_covers_at_least_the_full_size_area() {
    let r = px(5, 7, 11, 13);
    // Level 1 halves coordinates: outer edges round outward.
    assert_eq!(r.at_level(1, [500, 400]), Some(px(2, 3, 6, 7)));
    assert_eq!(r.at_level(0, [500, 400]), Some(r));
    // A very deep level is clamped to the level size.
    assert_eq!(r.at_level(40, [1, 1]), Some(px(0, 0, 1, 1)));
}

#[test]
fn touching_rectangles_do_not_intersect() {
    assert!(!px(0, 0, 10, 10).intersects(&px(10, 0, 20, 10)));
    assert!(px(0, 0, 10, 10).intersects(&px(9, 9, 20, 20)));
}

#[test]
fn no_markers_no_runs() {
    assert!(plan(&[None, None], &[], 8, SCREEN).is_empty());
}

#[test]
fn cards_side_by_side_share_one_run() {
    // Jobs: card A marker, A content, card B marker, B content. B is far from A's padded region.
    let a = px(0, 0, 100, 100);
    let b = px(500, 0, 600, 100);
    let jobs = [Some(a), Some(a), Some(b), Some(b)];
    let runs = plan(&jobs, &[marker(0, a), marker(2, b)], 20, SCREEN);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].start_job, 0);
    assert_eq!(runs[0].members.iter().map(|m| m.job).collect::<Vec<_>>(), [0, 2]);
}

#[test]
fn a_card_over_a_card_starts_a_new_run() {
    let a = px(0, 0, 200, 200);
    let b = px(100, 100, 300, 300);
    let jobs = [Some(a), Some(a), Some(b), Some(b)];
    let runs = plan(&jobs, &[marker(0, a), marker(2, b)], 10, SCREEN);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].start_job, 2);
}

#[test]
fn a_card_whose_padding_reaches_the_other_cards_pixels_does_not_join() {
    // Rectangles are 25 apart: disjoint, but B's padded region (pad 30) reaches A's pixels.
    let a = px(0, 0, 100, 100);
    let b = px(125, 0, 225, 100);
    let jobs = [Some(a), Some(b)];
    assert_eq!(plan(&jobs, &[marker(0, a), marker(1, b)], 30, SCREEN).len(), 2);
    assert_eq!(plan(&jobs, &[marker(0, a), marker(1, b)], 20, SCREEN).len(), 1);
}

#[test]
fn painting_between_the_markers_splits_the_run_only_where_it_reaches() {
    let a = px(0, 0, 100, 100);
    let b = px(500, 500, 600, 600);
    let near_b = px(480, 480, 520, 520);
    let far = px(300, 0, 340, 40);
    let jobs = [Some(a), None, Some(far), Some(b)];
    assert_eq!(plan(&jobs, &[marker(0, a), marker(3, b)], 20, SCREEN).len(), 1);
    let jobs = [Some(a), None, Some(near_b), Some(b)];
    assert_eq!(plan(&jobs, &[marker(0, a), marker(3, b)], 20, SCREEN).len(), 2);
}

#[test]
fn paint_before_the_snapshot_does_not_matter() {
    let a = px(0, 0, 100, 100);
    let under = px(0, 0, 1000, 800);
    let jobs = [Some(under), Some(a), Some(a)];
    assert_eq!(plan(&jobs, &[marker(1, a)], 10, SCREEN).len(), 1);
}

#[test]
fn only_the_current_run_can_be_joined() {
    // A, then B over A (new run), then C far from everything: C joins B's run, not A's.
    let a = px(0, 0, 200, 200);
    let b = px(100, 100, 300, 300);
    let c = px(700, 600, 800, 700);
    let jobs = [Some(a), Some(b), Some(c)];
    let runs = plan(&jobs, &[marker(0, a), marker(1, b), marker(2, c)], 10, SCREEN);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].members.len(), 1);
    assert_eq!(runs[1].members.iter().map(|m| m.job).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn members_get_their_padded_region_inside_the_screen() {
    let a = px(10, 10, 60, 60);
    let runs = plan(&[Some(a)], &[marker(0, a)], 25, SCREEN);
    assert_eq!(runs[0].members[0].region, px(0, 0, 85, 85));
    assert_eq!(runs[0].members[0].region.area(), 85 * 85);
}

#[test]
fn hostile_markers_are_dropped_or_reordered_without_panicking() {
    let a = px(0, 0, 10, 10);
    let jobs = [Some(a), Some(a)];
    // A job past the end, and markers out of order.
    let runs = plan(&jobs, &[marker(9, a), marker(1, a), marker(0, a)], u32::MAX, SCREEN);
    assert_eq!(runs.iter().map(|r| r.members.len()).sum::<usize>(), 2);
    // A marker wholly off a tiny screen.
    assert!(plan(&jobs, &[marker(0, px(500, 500, 600, 600))], 3, [10, 10]).is_empty());
}

#[test]
fn extents_apply_the_clip_and_round_outward() {
    let bounds = Rect::from_min_max(pos2(10.2, 10.2), pos2(50.1, 50.1));
    let clip = Rect::from_min_max(pos2(20.0, 0.0), pos2(1000.0, 30.5));
    assert_eq!(px_extent(bounds, clip, 2.0, [200, 200]), Some(px(40, 20, 101, 61)));
    // Clipped away entirely.
    assert_eq!(px_extent(bounds, Rect::from_min_max(pos2(60.0, 60.0), pos2(70.0, 70.0)), 1.0, [200, 200]), None);
    // Non finite values and bad scales do not panic.
    assert_eq!(px_extent(bounds, clip, 0.0, [200, 200]), None);
    assert_eq!(px_extent(bounds, clip, f32::NAN, [200, 200]), None);
    let nan = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1.0, 1.0));
    assert_eq!(px_extent(nan, Rect::EVERYTHING, 1.0, [10, 10]), None);
}

#[test]
fn reach_grows_with_steps_and_offset_and_survives_bad_input() {
    assert!(reach(3, 1.0) > reach(2, 1.0));
    assert!(reach(3, 2.0) > reach(3, 1.0));
    assert_eq!(reach(3, f32::NAN), 0);
    assert_eq!(reach(3, -1.0), 0);
    assert!(reach(usize::MAX, f32::MAX) > 0);
}

fn run_of(jobs: &[(usize, PxRect)]) -> Run {
    Run { start_job: jobs[0].0, members: jobs.iter().map(|(job, region)| Member { job: *job, region: *region }).collect() }
}

#[test]
fn a_budget_drops_the_markers_that_do_not_fit_and_keeps_the_rest() {
    let big = px(0, 0, 100, 100);
    let small = px(200, 0, 210, 10);
    let runs = vec![run_of(&[(0, big), (2, small)]), run_of(&[(4, big)]), run_of(&[(6, small)])];
    let budget = Budget { area: 10_000 + 100 + 100, runs: 8, members: 8 };
    let kept = within_budget(runs, budget);
    // The second `big` does not fit; the small ones do.
    let jobs: Vec<Vec<usize>> = kept.iter().map(|r| r.members.iter().map(|m| m.job).collect()).collect();
    assert_eq!(jobs, [vec![0, 2], vec![6]]);
    assert_eq!(kept[1].start_job, 6, "a run starts at its first kept member");
}

#[test]
fn a_budget_caps_runs_and_members() {
    let r = px(0, 0, 4, 4);
    let runs: Vec<Run> = (0..5).map(|i| run_of(&[(i * 2, r), (i * 2 + 1, r)])).collect();
    assert_eq!(within_budget(runs.clone(), Budget { area: u64::MAX, runs: 3, members: 100 }).len(), 3);
    let kept = within_budget(runs, Budget { area: u64::MAX, runs: 100, members: 3 });
    assert_eq!(kept.iter().map(|r| r.members.len()).sum::<usize>(), 3);
    assert!(within_budget(Vec::new(), Budget::for_screen([10, 10])).is_empty());
    assert_eq!(Budget::for_screen([u32::MAX, u32::MAX]).area, u64::MAX);
}
