use super::*;

fn linear(length: f64) -> Part {
    Part { length, width: 1.0, rotation: Rotation::Fixed }
}

#[test]
fn rafters_blocks_kerf_trim_and_gross_yield_are_accounted_for() {
    let parts = [1966.0, 1966.0, 1966.0, 300.0, 300.0].map(linear);
    let stocks = [Stock { length: 2400.0, width: 1.0 }];
    let settings = Settings { linear: true, kerf: 3.0, trim: 5.0 };
    let p = pack(&parts, &stocks, settings).unwrap();
    assert_eq!(p.sheets.len(), 3);
    assert!(p.unplaced.is_empty());
    // Two boards: 5 + 1966 + 3 + 300 + 3 + 118 + 5 = 2400.
    // Third board: 5 + 1966 + 3 + 421 + 5 = 2400.
    let leftovers: Vec<_> = p.sheets.iter().flat_map(|s| &s.offcuts).map(|r| r.length).collect();
    assert_eq!(leftovers, [118.0, 118.0, 421.0]);
    for (s, length) in p.sheets.iter().zip([2266.0, 2266.0, 1966.0]) {
        assert!((s.yield_ratio - length / 2400.0).abs() < 1e-12);
    }
    assert_eq!(p, pack(&parts, &stocks, settings).unwrap());
    let halves = pack(&[linear(1200.0), linear(1200.0)], &stocks, Settings { trim: 0.0, ..settings }).unwrap();
    assert_eq!(halves.sheets.len(), 2);
    assert_eq!(halves.sheets[0].offcuts[0].length, 1197.0);
}

#[test]
fn rfc_bench_uses_four_sheets_or_five_with_grain() {
    let stocks = [Stock { length: 2440.0, width: 1220.0 }];
    let settings = Settings { linear: false, kerf: 3.2, trim: 10.0 };
    for (rotation, count) in [(Rotation::Free, 4), (Rotation::Fixed, 5)] {
        let mut parts = Vec::new();
        for (length, width, n) in [(1828.8, 762.0, 2), (878.4, 591.6, 3), (1676.4, 778.4, 1), (811.2, 591.6, 2)] {
            parts.extend((0..n).map(|_| Part { length, width, rotation }));
        }
        let plan = pack(&parts, &stocks, settings).unwrap();
        assert_eq!(plan.sheets.len(), count, "{plan:#?}");
        assert!(plan.unplaced.is_empty());
        assert_eq!(plan.sheets.iter().map(|s| s.placements.len()).sum::<usize>(), 8);
        let area = 2.0 * 1828.8 * 762.0 + 3.0 * 878.4 * 591.6 + 1676.4 * 778.4 + 2.0 * 811.2 * 591.6;
        let yield_ratio = plan.sheets.iter().map(|s| s.yield_ratio).sum::<f64>() / count as f64;
        assert!((yield_ratio - area / (count as f64 * 2440.0 * 1220.0)).abs() < 1e-12);
        if count == 4 {
            assert!((yield_ratio - 0.5550).abs() < 0.001);
        }
        for sheet in &plan.sheets {
            let occupied =
                sheet.placements.iter().map(|p| p.rect).chain(sheet.offcuts.iter().copied()).chain(sheet.cuts.iter().copied()).collect::<Vec<_>>();
            let total = occupied.iter().map(|r| r.length * r.width).sum::<f64>();
            assert!((total - 2420.0 * 1200.0).abs() < 1e-7);
            for (i, a) in occupied.iter().enumerate() {
                assert!(a.x >= 10.0 && a.y >= 10.0 && a.x + a.length <= 2430.0 + 1e-9 && a.y + a.width <= 1210.0 + 1e-9);
                for b in occupied.iter().skip(i + 1) {
                    let overlap = (a.x + a.length).min(b.x + b.length) - a.x.max(b.x);
                    let height = (a.y + a.width).min(b.y + b.width) - a.y.max(b.y);
                    assert!(overlap < 1e-8 || height < 1e-8);
                }
            }
        }
        assert_eq!(plan, pack(&parts, &stocks, settings).unwrap());
    }
}

#[test]
fn rotation_stock_choices_unplaced_and_hostile_input() {
    let settings = Settings { linear: false, kerf: 1.0, trim: 0.0 };
    let stocks = [Stock { length: 60.0, width: 30.0 }, Stock { length: 100.0, width: 50.0 }];
    let mixed = pack(
        &[Part { length: 90.0, width: 45.0, rotation: Rotation::Fixed }, Part { length: 58.0, width: 28.0, rotation: Rotation::Fixed }],
        &stocks,
        Settings { kerf: 3.0, ..settings },
    )
    .unwrap();
    assert_eq!(mixed.sheets.iter().map(|s| s.stock).collect::<Vec<_>>(), [1, 0]);
    assert!(mixed.unplaced.is_empty());
    let part = Part { length: 40.0, width: 80.0, rotation: Rotation::Free };
    let p = pack(std::slice::from_ref(&part), &stocks, settings).unwrap();
    assert_eq!(p.sheets[0].stock, 1);
    assert!(p.sheets[0].placements[0].rotated);
    let locked = Part { rotation: Rotation::Fixed, ..part };
    assert_eq!(pack(&[locked], &stocks, settings).unwrap().unplaced, [0]);
    assert_eq!(pack(&[linear(120.0)], &stocks, settings).unwrap().unplaced, [0]);
    assert_eq!(pack(&[], &stocks, settings).unwrap(), Plan::default());
    assert_eq!(pack(&[linear(1.0)], &[], settings).unwrap().unplaced, [0]);
    for kerf in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(pack(&[], &stocks, Settings { kerf, ..settings }).is_err());
    }
    for length in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e30] {
        assert!(pack(&[linear(length)], &stocks, settings).is_err());
    }
    assert!(pack(&vec![linear(1.0); 2049], &stocks, settings).is_err());
}
