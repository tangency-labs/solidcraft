//! The ribbon's fit, from plain group sizes.
use super::*;

fn sizes(buttons: &[usize]) -> Vec<PanelSize> {
    buttons.iter().map(|&b| PanelSize { buttons: vec![ButtonSize { width: BUTTON_MIN_WIDTH, min_width: BUTTON_MIN_WIDTH }; b] }).collect()
}

fn used_with_more(plans: &[Plan]) -> f32 {
    used(plans) + if plans.iter().any(|p| p.fit == Fit::More) { MORE_ROOM } else { 0.0 }
}

#[test]
fn panels_give_up_one_button_at_a_time_then_move_to_more_and_none_is_dropped() {
    // A panel with 14 buttons and three default ones.
    let panels = sizes(&[14, 4, 2, 2]);
    let full: f32 = [14, 4, 2, 2].iter().map(|&b| panel_width(&vec![BUTTON_MIN_WIDTH; b]) + PANEL_GAP).sum();
    let all = |plans: &[Plan]| plans.iter().map(|p| (p.fit, p.shown)).collect::<Vec<_>>();
    assert_eq!(all(&fit(&panels, full)), [(Fit::Buttons, 14), (Fit::Buttons, 4), (Fit::Buttons, 2), (Fit::Buttons, 2)]);
    // A little short: the widest panel hands one button to its drop-down; the others keep all theirs.
    assert_eq!(all(&fit(&panels, full - 10.0)), [(Fit::Buttons, 13), (Fit::Buttons, 4), (Fit::Buttons, 2), (Fit::Buttons, 2)]);
    // Every panel down to one button (collapsed) fits.
    let collapsed = 4.0 * (COLLAPSED_WIDTH + PANEL_GAP);
    assert_eq!(all(&fit(&panels, collapsed)), [(Fit::Collapsed, 1); 4]);
    // Narrower: the last one goes into the » menu, and the room it frees goes back to buttons.
    let more = fit(&panels, collapsed - 1.0);
    assert_eq!(more.iter().map(|p| p.fit).collect::<Vec<_>>(), [Fit::Collapsed, Fit::Collapsed, Fit::Collapsed, Fit::More]);
    assert!(more.iter().take(3).all(|p| p.shown >= 1));
    let more = fit(&sizes(&[14, 4, 2, 2, 2, 2]), 5.0 * (COLLAPSED_WIDTH + PANEL_GAP) + MORE_ROOM - 1.0);
    assert_eq!(more.last().unwrap().fit, Fit::More);
    assert!(more.iter().any(|p| p.fit == Fit::Buttons && p.shown > 1), "overflow room restores buttons");
    // Then all of them.
    assert!(fit(&panels, 0.0).iter().all(|p| p.fit == Fit::More));
    assert_eq!(fit(&panels, f32::NAN).len(), 4);
    // No cap: every additional button keeps its width and gap.
    assert_eq!(panel_width(&[BUTTON_MIN_WIDTH; 30]) - panel_width(&[BUTTON_MIN_WIDTH; 20]), 10.0 * (BUTTON_MIN_WIDTH + BUTTON_GAP));
}

/// #2108: whatever the room, the toolbar uses it. It never leaves more than one button's width
/// blank while something is hidden, never runs past the room, and shrinks steadily as the
/// room does (so a few pixels never flip it back and forth).
#[test]
fn panels_fill_the_room_they_have_at_every_width() {
    let layouts = [
        // SOLID: CREATE, MODIFY, ASSEMBLE, CONFIGURE, CONSTRUCT, INSPECT, INSERT, SELECT.
        vec![(4, false), (4, false), (3, false), (2, false), (2, false), (2, false), (2, false), (0, false)],
        // SKETCH: labelled CREATE, then MODIFY … FINISH SKETCH.
        vec![(8, true), (2, false), (3, false), (6, false), (2, false), (2, false), (0, false), (1, false)],
        // Many pins on one panel.
        vec![(14, false), (4, false), (2, false), (2, false)],
    ];
    for layout in layouts {
        let panels: Vec<PanelSize> = layout
            .iter()
            .map(|&(buttons, labelled)| PanelSize {
                buttons: (0..buttons)
                    .map(|i| ButtonSize {
                        width: if labelled { BUTTON_MAX_WIDTH - (i % 3) as f32 * 5.0 } else { BUTTON_MIN_WIDTH },
                        min_width: BUTTON_MIN_WIDTH,
                    })
                    .collect(),
            })
            .collect();
        let mut last: Option<(f32, usize)> = None;
        for room in (0..=2000).rev().map(|r| r as f32) {
            let plans = fit(&panels, room);
            let width = used_with_more(&plans);
            let hidden = plans
                .iter()
                .zip(&panels)
                .any(|(p, s)| p.fit != Fit::Buttons || p.shown < s.buttons.len() || p.widths.iter().zip(&s.buttons).any(|(w, b)| *w < b.width));
            let more = plans.iter().filter(|p| p.fit == Fit::More).count();
            if more < panels.len() {
                assert!(width <= room + 0.01, "{layout:?} at {room}: {width} overruns");
            }
            // One button: an icon's stride, or a collapsed panel's when the next panel waits in the » menu.
            let button = if more > 0 { COLLAPSED_WIDTH + PANEL_GAP } else { BUTTON_MIN_WIDTH + BUTTON_GAP };
            if hidden && more < panels.len() {
                assert!(room - width < button, "{layout:?} at {room}: {} px blank while something is hidden", room - width);
            }
            // Fewer pixels never bring a panel back out of the » menu.
            if let Some((_, prev_more)) = last {
                assert!(more >= prev_more, "{layout:?} at {room}: a panel came back out of the » menu");
            }
            last = Some((width, more));
            // A panel shows its icons: one at the very least.
            for (p, s) in plans.iter().zip(&panels) {
                if p.fit != Fit::More && !s.buttons.is_empty() {
                    assert!(p.shown >= 1, "{layout:?} at {room}: a panel without icons");
                }
            }
        }
    }
}

#[test]
fn variable_width_buttons_narrow_before_rightmost_equal_panels_lose_buttons() {
    let panels = vec![PanelSize {
        buttons: vec![
            ButtonSize { width: 76.0, min_width: BUTTON_MIN_WIDTH },
            ButtonSize { width: 63.0, min_width: BUTTON_MIN_WIDTH },
            ButtonSize { width: 54.0, min_width: BUTTON_MIN_WIDTH },
        ],
    }];
    let full = used(&panels.iter().map(Plan::full).collect::<Vec<_>>());
    let narrow = fit(&panels, full - 1.0);
    assert_eq!(narrow[0].shown, 3);
    assert_eq!(narrow[0].widths, [75.0, 62.0, 54.0]);
    let minimum = panel_width(&[BUTTON_MIN_WIDTH; 3]) + PANEL_GAP;
    let narrow = fit(&panels, minimum);
    assert_eq!(narrow[0].shown, 3);
    assert_eq!(narrow[0].widths, [BUTTON_MIN_WIDTH; 3]);
    assert_eq!(fit(&panels, minimum - 1.0)[0].shown, 2);

    let equal = sizes(&[3, 3]);
    let full = used(&equal.iter().map(Plan::full).collect::<Vec<_>>());
    assert_eq!(fit(&equal, full - 1.0).iter().map(|p| p.shown).collect::<Vec<_>>(), [3, 2]);
}

#[test]
fn constraint_gallery_stays_fixed_and_hands_all_cells_to_the_menu_together() {
    let panels = vec![PanelSize {
        buttons: vec![
            ButtonSize { width: BUTTON_MAX_WIDTH, min_width: BUTTON_MIN_WIDTH },
            ButtonSize { width: GALLERY_WIDTH, min_width: GALLERY_WIDTH },
        ],
    }];
    let minimum = panel_width(&[BUTTON_MIN_WIDTH, GALLERY_WIDTH]) + PANEL_GAP;
    let narrow = fit(&panels, minimum);
    assert_eq!(narrow[0].shown, 2);
    assert_eq!(narrow[0].widths, [BUTTON_MIN_WIDTH, GALLERY_WIDTH]);
    let collapsed = fit(&panels, minimum - 1.0);
    assert_eq!((collapsed[0].fit, collapsed[0].shown), (Fit::Collapsed, 1));
}
