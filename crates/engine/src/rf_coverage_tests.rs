use super::*;
use solidcraft_doc::rf::rf_preset;

fn preset(id: &str) -> (String, RfLoss) {
    (id.to_string(), rf_preset(id).map(|p| p.loss).unwrap_or(RfLoss([0.0; 3])))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn wall(x0: f64, y0: f64, x1: f64, y1: f64, material: usize) -> WallRegion {
    WallRegion::new(vec![rect(x0, y0, x1, y1)], material, "wall".into()).unwrap()
}

fn emitter(at: [f64; 2], eirp: f64) -> Emitter {
    Emitter { name: "AP".into(), at, height: 1000.0, eirp_dbm: [Some(eirp); 3] }
}

const MODEL: Model = Model { exponent: 2.2, client_height: 1000.0 };

/// FSPL at 1 m: 20·log10(5500) − 27.55 = 74.807 − 27.55 = 47.257 dB.
#[test]
fn a_free_space_point_follows_the_log_distance_law() {
    assert!((fspl_1m(Band::G5) - 47.257).abs() < 1e-3);
    let walls = WallMap::default();
    let mut scratch = Scratch::default();
    // 10 m away at 30 dBm: 30 − 47.257 − 22·log10(10) = −39.257 dBm.
    let (v, _) = best_at(&walls, &[emitter([0.0, 0.0], 30.0)], &MODEL, Band::G5, [10_000.0, 0.0], &mut scratch).unwrap();
    assert!((v - (-39.257)).abs() < 1e-3, "{v}");
    // The 3D distance counts: 6 m across and 8 m down is 10 m.
    let high = Emitter { height: 9000.0, ..emitter([0.0, 0.0], 30.0) };
    let (v3, _) = best_at(&walls, &[high], &MODEL, Band::G5, [6000.0, 0.0], &mut scratch).unwrap();
    assert!((v3 - v).abs() < 1e-9);
    // Closer than 0.3 m counts as 0.3 m.
    assert_eq!(rssi(30.0, Band::G5, 2.2, 0.0, 0.0), rssi(30.0, Band::G5, 2.2, 0.3, 0.0));
}

#[test]
fn one_concrete_wall_costs_its_loss_once_per_band() {
    let walls = WallMap::new(vec![wall(4000.0, -5000.0, 4200.0, 5000.0, 0)], vec![preset("concrete")]);
    let mut scratch = Scratch::default();
    for (band, loss) in [(Band::G2_4, 12.0), (Band::G5, 20.0), (Band::G6, 24.0)] {
        assert_eq!(walls.path_loss([0.0, 0.0], [10_000.0, 0.0], band, &mut scratch), loss);
        // A path that stays on one side crosses nothing.
        assert_eq!(walls.path_loss([0.0, 0.0], [3000.0, 2000.0], band, &mut scratch), 0.0);
    }
    let open = WallMap::default();
    let free = best_at(&open, &[emitter([0.0, 0.0], 30.0)], &MODEL, Band::G5, [10_000.0, 0.0], &mut scratch).unwrap().0;
    let behind = best_at(&walls, &[emitter([0.0, 0.0], 30.0)], &MODEL, Band::G5, [10_000.0, 0.0], &mut scratch).unwrap().0;
    assert!((free - behind - 20.0).abs() < 1e-9);
}

#[test]
fn a_glass_pane_thinner_than_the_grid_still_counts() {
    // 6 mm low-E glazing.
    let walls = WallMap::new(vec![wall(3000.0, -2000.0, 3006.0, 2000.0, 0)], vec![preset("low-e-glass")]);
    let mut scratch = Scratch::default();
    assert_eq!(walls.path_loss([0.0, 0.0], [6000.0, 500.0], Band::G5, &mut scratch), 15.0);
    assert_eq!(walls.path_loss([0.0, 0.0], [6000.0, 500.0], Band::G6, &mut scratch), 18.0);
    // A 75 mm grid misses the pane's cells by their centres but still closes the room at it.
    let grid = Grid::covering([0.0, -2000.0, 6000.0, 2000.0], 75.0).unwrap();
    assert!(wall_cells(&walls, &grid).iter().all(|w| !w));
}

#[test]
fn abutting_boxes_of_one_wall_are_one_crossing_and_overlaps_take_the_larger_loss() {
    // One drywall wall built from two boxes that meet along the path.
    let walls = WallMap::new(
        vec![wall(1000.0, -100.0, 1045.0, 100.0, 0), wall(1045.0, -100.0, 1090.0, 100.0, 0)],
        vec![preset("drywall"), preset("wood-door"), preset("concrete")],
    );
    let mut scratch = Scratch::default();
    assert_eq!(walls.path_loss([0.0, 0.0], [2000.0, 0.0], Band::G5, &mut scratch), 4.0);
    // Drywall then concrete straight after it: two runs.
    let two = WallMap::new(
        vec![wall(1000.0, -100.0, 1090.0, 100.0, 0), wall(1090.0, -100.0, 1290.0, 100.0, 2)],
        vec![preset("drywall"), preset("wood-door"), preset("concrete")],
    );
    assert_eq!(two.path_loss([0.0, 0.0], [2000.0, 0.0], Band::G5, &mut scratch), 24.0);
    // A door inside a concrete opening's box: only the concrete counts where both are.
    let overlap = WallMap::new(
        vec![wall(1000.0, -100.0, 1200.0, 100.0, 2), wall(1050.0, -50.0, 1150.0, 50.0, 1)],
        vec![preset("drywall"), preset("wood-door"), preset("concrete")],
    );
    assert_eq!(overlap.path_loss([0.0, 0.0], [2000.0, 0.0], Band::G5, &mut scratch), 20.0);
    // An emitter inside a wall isn't charged for leaving it; the next wall counts.
    assert_eq!(two.path_loss([1050.0, 0.0], [2000.0, 0.0], Band::G5, &mut scratch), 20.0);
    // Through a corner exactly: one crossing, not two.
    let corner = WallMap::new(vec![wall(1000.0, 1000.0, 2000.0, 2000.0, 2)], vec![preset("drywall"), preset("wood-door"), preset("concrete")]);
    assert_eq!(corner.path_loss([0.0, 0.0], [3000.0, 3000.0], Band::G5, &mut scratch), 20.0);
}

/// Two rooms 4 m × 3 m side by side, 150 mm concrete outside and a 90 mm drywall partition.
fn two_rooms() -> WallMap {
    let walls = vec![
        wall(-150.0, -150.0, 8240.0, 0.0, 0),
        wall(-150.0, 3000.0, 8240.0, 3150.0, 0),
        wall(-150.0, 0.0, 0.0, 3000.0, 0),
        wall(8090.0, 0.0, 8240.0, 3000.0, 0),
        wall(4000.0, 0.0, 4090.0, 3000.0, 1),
    ];
    WallMap::new(walls, vec![preset("concrete"), preset("drywall")])
}

#[test]
fn the_enclosed_floor_and_its_stats() {
    let walls = two_rooms();
    let grid = Grid::covering(walls.bounds().unwrap(), 75.0).unwrap();
    let solid = wall_cells(&walls, &grid);
    let inside = enclosed(&walls, &grid, &solid);
    let cells: Vec<Cell> = solid
        .iter()
        .zip(&inside)
        .map(|(w, i)| {
            if *w {
                Cell::Wall
            } else if *i {
                Cell::Floor
            } else {
                Cell::Outside
            }
        })
        .collect();
    let floor = cells.iter().filter(|c| **c == Cell::Floor).count() as f64 * 75.0 * 75.0 / 1e6;
    // 2 × 12 m², less the cells the walls' edges touch.
    assert!((20.0..24.0).contains(&floor), "{floor}");
    let e = [emitter([2000.0, 1500.0], 30.0)];
    let values = coverage(&walls, &e, &MODEL, Band::G5, &grid, &cells);
    let s = stats(&values, &cells).unwrap();
    // The far room is behind 4 dB of drywall: the weakest corner is 6.4 m away, about
    // 30 − 47.26 − 22·log10(6.4) − 4 = −39 dBm, so everything is good.
    assert_eq!(s.good, 1.0);
    assert!((s.min - (30.0 - fspl_1m(Band::G5) - 22.0 * (6.09f64.hypot(1.46) / 1.0).log10() - 4.0)).abs() < 1.0, "{s:?}");
    assert!(s.p5 <= s.median && s.median <= s.max);
    // A wall-less plan encloses nothing.
    let open = WallMap::new(vec![wall(0.0, 0.0, 100.0, 100.0, 0)], vec![preset("concrete")]);
    let g = Grid::covering([-1000.0, -1000.0, 1000.0, 1000.0], 75.0).unwrap();
    assert!(enclosed(&open, &g, &wall_cells(&open, &g)).iter().all(|i| !i));
}

#[test]
fn hostile_inputs_never_panic() {
    assert!(Grid::covering([0.0, 0.0, 1e12, 1e12], 1.0).is_none());
    assert!(Grid::covering([0.0, 0.0, f64::NAN, 1.0], 1.0).is_none());
    assert!(Grid::covering([0.0, 0.0, 1.0, 1.0], 0.0).is_none());
    assert!(WallRegion::new(vec![vec![[0.0, 0.0], [1.0, f64::NAN], [2.0, 0.0]]], 0, "x".into()).is_none());
    let walls = WallMap::new(vec![wall(0.0, 0.0, 1.0, 1.0, 7)], vec![preset("concrete")]);
    assert!(walls.regions.is_empty());
    let mut scratch = Scratch::default();
    assert_eq!(two_rooms().path_loss([1.0, 1.0], [1.0, 1.0], Band::G5, &mut scratch), 0.0);
    // Through the outer corner and the far wall: two concrete crossings.
    assert_eq!(two_rooms().path_loss([-1e9, -1e9], [1e9, 1e9], Band::G5, &mut scratch), 40.0);
    assert!(stats(&[f32::NAN], &[Cell::Floor]).is_none());
    assert_eq!(colour(f64::NAN), [120, 120, 130]);
}

#[test]
fn one_body_of_thin_and_thick_walls_is_guessed_where_each_path_crosses() {
    // One region: a 90 mm partition (x 0…90) joined to a 200 mm wall (y 3000…3200), an L.
    let l = vec![[0.0, 0.0], [90.0, 0.0], [90.0, 3000.0], [4000.0, 3000.0], [4000.0, 3200.0], [0.0, 3200.0]];
    let region = WallRegion::new(vec![l], 0, "Walls".into()).unwrap().guessed(0, 1, solidcraft_doc::rf::CONCRETE_FROM_MM);
    assert!((region.thickness_at([45.0, 1000.0]) - 90.0).abs() < 1e-9);
    let walls = WallMap::new(vec![region], vec![preset("drywall"), preset("concrete")]);
    let mut scratch = Scratch::default();
    // Square through the partition, and slanting through it: drywall either way.
    assert_eq!(walls.path_loss([-1000.0, 1000.0], [1000.0, 1000.0], Band::G5, &mut scratch), 4.0);
    assert_eq!(walls.path_loss([-1000.0, 500.0], [1000.0, 1800.0], Band::G5, &mut scratch), 4.0);
    // Through the thick wall: concrete.
    assert_eq!(walls.path_loss([2000.0, 2000.0], [2500.0, 4000.0], Band::G5, &mut scratch), 20.0);
}
