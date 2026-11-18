#[cfg(test)]
mod tests {
    use super::*;

    fn run(rain: &mut Rain, seconds: f32) {
        let start = rain.last.unwrap_or(0.0);
        let mut t = 0.0;
        while t < seconds {
            t += 1.0 / 30.0;
            rain.advance(start + t, rain.aspect);
        }
    }

    /// Small drops land and stay; some grow by merging until they slide.
    #[test]
    fn the_window_gets_wet_and_some_drops_slide() {
        let mut rain = Rain::new(1.6, 7, AMOUNT_AT_LANDING);
        let resting = rain.drops.iter().filter(|d| !d.sliding).count();
        assert!(resting > 300, "spray at rest: {resting}");
        let mut slid = 0;
        for _ in 0..60 {
            run(&mut rain, 0.5);
            slid = slid.max(rain.sliding());
        }
        assert!(slid >= 3, "drops slide: {slid}");
        assert!(rain.drops.len() <= MAX_DROPS);
    }

    /// Two drops that meet are one with both's water, and a sliding one goes faster.
    #[test]
    fn drops_that_meet_become_one_and_speed_up() {
        let at_rest = Drop { p: [0.5, 0.5], r: 0.005, vy: 0.0, vx: 0.0, sliding: false, since_bead: 0.0, from_y: 0.5, age: 1.0, id: 0 };
        let sliding = Drop { p: [0.5, 0.494], r: 0.008, vy: 0.2, sliding: true, ..at_rest };
        let one = joined(at_rest, sliding);
        assert!((one.water() - at_rest.water() - sliding.water()).abs() < 1e-12);
        assert!(one.sliding && one.vy > sliding.vy);
        let two_small = joined(at_rest, Drop { p: [0.503, 0.5], ..at_rest });
        assert!(two_small.sliding == (two_small.r >= R_STICK));
    }

    /// Heavier drops slide faster.
    #[test]
    fn heavier_is_faster() {
        let mut rain = Rain::new(1.0, 3, AMOUNT_AT_LANDING);
        rain.drops.clear();
        for (x, r) in [(0.3, 0.008), (0.7, 0.014)] {
            rain.drops.push(Drop { p: [x, 0.05], r, vy: 0.0, vx: 0.0, sliding: true, since_bead: 0.0, from_y: 0.05, age: 1.0, id: 0 });
        }
        for _ in 0..15 {
            rain.step(1.0 / 30.0);
        }
        let speed = |x: f32| rain.drops.iter().filter(|d| d.sliding && (d.p[0] - x).abs() < 0.1).map(|d| d.vy).fold(0.0, f32::max);
        assert!(speed(0.7) > speed(0.3) * 1.3, "{} vs {}", speed(0.7), speed(0.3));
    }

    /// Nearly every sliding drop reaches the bottom; a small one from high up may run dry.
    #[test]
    fn sliding_drops_mostly_reach_the_bottom() {
        let mut rain = Rain::new(1.6, 11, AMOUNT_AT_LANDING);
        run(&mut rain, 120.0);
        let Fates { reached, stuck, stuck_high_starters } = rain.fates;
        assert!(reached >= 10, "drops reach the bottom: {:?}", rain.fates);
        assert!(reached as f32 / (reached + stuck) as f32 > 0.85, "{:?}", rain.fates);
        assert!(stuck_high_starters <= stuck);
    }

    /// The trail a drop wipes fogs over again, slowly.
    #[test]
    fn a_wiped_trail_fogs_over() {
        let mut wiped = vec![0.0; (WIPED_W * WIPED_H) as usize];
        stamp(&mut wiped, 1.0, [0.5, 0.2], [0.5, 0.3], 0.006);
        let at = (0.25 * WIPED_H as f32) as usize * WIPED_W as usize + WIPED_W as usize / 2;
        assert!(wiped[at] > 0.9);
        let mut rain = Rain::new(1.0, 5, AMOUNT_AT_LANDING);
        rain.drops.clear();
        rain.wiped = wiped;
        for _ in 0..30 {
            rain.step(0.1);
        }
        assert!(rain.wiped[at] < 0.9 && rain.wiped[at] > 0.0, "{}", rain.wiped[at]);
    }

    /// The drops go to the shader sorted by cell, each cell pointing at its own.
    #[test]
    fn the_upload_finds_each_drop_in_its_cell() {
        let rain = Rain::new(1.6, 9, AMOUNT_AT_LANDING);
        let up = rain.upload();
        assert_eq!(up.drops.len(), rain.drops.len());
        for (c, cell) in up.cells.iter().enumerate() {
            let (cx, cy) = ((c as u32 % CELLS_W) as f32, (c as u32 / CELLS_W) as f32);
            for d in &up.drops[cell[0] as usize..(cell[0] + cell[1]) as usize] {
                assert!((d[0] / CELL).floor().max(0.0) == cx || cx as u32 == ((1.6 / CELL).ceil() as u32 - 1), "x {d:?} in {cx}");
                assert!((d[1] / CELL).floor().clamp(0.0, (CELLS_H - 1) as f32) == cy, "y {d:?} in {cy}");
            }
        }
    }

    /// No drop just vanishes: one leaves the glass only by merging into another or running off the
    /// bottom; none dries away. A full window takes no new drops instead of taking old ones away.
    #[test]
    fn no_drop_vanishes_without_a_reason() {
        for amount in [AMOUNT_AT_LANDING, DEFAULT_AMOUNT, 1.0] {
            let mut rain = Rain::new(1.6, 21, amount);
            let dt = 1.0 / 30.0;
            let mut t = 0.0;
            let (mut merged, mut off) = (0, 0);
            for _ in 0..1800 {
                let before: std::collections::HashMap<u32, Drop> = rain.drops.iter().map(|d| (d.id, *d)).collect();
                rain.gone.clear();
                t += dt;
                rain.advance(t, 1.6);
                assert!(rain.drops.len() <= MAX_DROPS, "{} drops", rain.drops.len());
                let now: std::collections::HashSet<u32> = rain.drops.iter().map(|d| d.id).collect();
                for (id, was) in &before {
                    if now.contains(id) {
                        continue;
                    }
                    let why = rain.gone.iter().find(|(g, _)| g == id).map(|&(_, why)| why);
                    match why {
                        Some(Gone::Merged) => merged += 1,
                        Some(Gone::OffTheEdge) => {
                            assert!(was.p[1] - was.r + was.vy * dt * 1.5 >= 0.98, "ran off from {was:?}");
                            off += 1;
                        }
                        None => panic!("drop {id} vanished: {was:?}"),
                    }
                }
            }
            assert!(merged > 0 && off > 0, "amount {amount}: merged {merged}, off {off}");
        }
    }

    /// A full window doesn't freeze: the rain joins the drops there, and they still run off.
    #[test]
    fn a_full_window_keeps_running() {
        let mut rain = Rain::new(1.6, 5, 1.0);
        let dt = 1.0 / 30.0;
        let mut t = 0.0;
        while rain.drops.len() < MAX_DROPS {
            t += dt;
            rain.advance(t, 1.6);
            assert!(t < 120.0, "the window fills");
        }
        let before = rain.fates.reached;
        for _ in 0..900 {
            t += dt;
            rain.advance(t, 1.6);
        }
        assert!(rain.fates.reached > before + 10, "full, drops still run off: {}", rain.fates.reached - before);
    }

    #[test]
    fn a_new_drop_grows_in() {
        let mut rain = Rain::new(1.0, 4, 0.0);
        let d = rain.new_drop([0.5, 0.5], 0.004);
        assert!(drawn_radius(&d) == 0.0);
        rain.drops.push(d);
        rain.step(1.0 / 30.0);
        let first = drawn_radius(rain.drops.iter().find(|x| x.id == d.id).unwrap());
        assert!(first > 0.0 && first < 0.003, "{first}");
        for _ in 0..6 {
            rain.step(1.0 / 30.0);
        }
        let later = drawn_radius(rain.drops.iter().find(|x| x.id == d.id).unwrap());
        assert!(later > 0.0039, "{later}");
    }

    #[test]
    fn more_rain_puts_more_drops_on_the_glass() {
        let light = Rain::new(1.6, 13, AMOUNT_AT_LANDING).drops.len();
        let default = Rain::new(1.6, 13, DEFAULT_AMOUNT).drops.len();
        let none = Rain::new(1.6, 13, 0.0).drops.len();
        assert!(none == 0 && default as f32 > light as f32 * 1.5, "none {none}, light {light}, default {default}");
    }
}
