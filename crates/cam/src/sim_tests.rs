//! Tests of the stock-removal simulation: the tool's swept envelope against closed forms and
//! brute force, a pocket's floor and walls, gouges and rapids caught, the limits, how long a
//! 50 × 50 mm part takes, and the simulation advanced a part at a time.

use std::time::Instant;

use solidcore_record::{Vec2, Vec3};

use crate::adaptive::HeightMap;
use crate::foundation_tests::*;
use crate::gcode5::{Post5, emit5};
use crate::kinematics::Machine;
use crate::sim::{Profile, SimParams, SimReport, SimState, StockGrid, StockShape, TILE, plan_sim, simulate, simulate_watched, stock_around};
use crate::*;

fn flat(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

fn ball(d: f64) -> Tool {
    Tool::new(1, ToolKind::BallEndmill, d, 20.0)
}

fn bull(d: f64, c: f64) -> Tool {
    Tool { corner_radius: Some(c), ..Tool::new(1, ToolKind::BullNose, d, 20.0) }
}

fn drill(d: f64) -> Tool {
    Tool::new(1, ToolKind::Drill, d, 20.0)
}

/// A program: the header a post writes, tool 1, then `moves`.
fn program(moves: &str) -> String {
    format!("%\nG90 G94 G91.1 G17\nG21\nT1 M6\nS12000 M3\nG0 X0. Y0.\nG43 Z10. H1\n{moves}\nM5\nG28 G91 Z0.\nG90\nM30\n%\n")
}

fn params(tol: f64, resolution: f64) -> SimParams {
    SimParams { tolerance: tol, resolution: Some(resolution) }
}

/// The 40 × 40 × 10 mm stock under work zero: top at Z0.
fn stock() -> StockShape {
    StockShape::Box { min: Vec3::new(0.0, 0.0, -10.0), max: Vec3::new(40.0, 40.0, 0.0) }
}

fn run(text: &str, tool: &Tool, part: &Surface, resolution: f64) -> SimReport {
    simulate(text, &Machine::three_axis(), std::slice::from_ref(tool), &stock(), part, &params(0.01, resolution)).unwrap()
}

/// The height of the cell whose centre is nearest `(x, y)`.
fn height_at(r: &SimReport, x: f64, y: f64) -> f64 {
    let StockGrid::Heights { x0, y0, dx, dy, nx, heights, .. } = &r.stock;
    let (i, j) = (((x - x0) / dx).floor() as usize, ((y - y0) / dy).floor() as usize);
    heights[j * nx + i]
}

/// Every cell centre and its height.
fn cells(r: &SimReport) -> Vec<(f64, f64, f64)> {
    let StockGrid::Heights { x0, y0, dx, dy, nx, heights, .. } = &r.stock;
    heights.iter().enumerate().map(|(c, h)| (x0 + ((c % nx) as f64 + 0.5) * dx, y0 + ((c / nx) as f64 + 0.5) * dy, *h)).collect()
}

/// The least height of the tool over `(u, v)` by brute force: the tip at many points along the line.
fn brute_line(p: &Profile, u: f64, v: f64, len: f64, z0: f64, dz: f64) -> Option<f64> {
    let n = 200_000;
    (0..=n)
        .filter_map(|k| {
            let s = len * k as f64 / n as f64;
            let d = (s - u).hypot(v);
            (d <= p.radius).then(|| z0 + dz * s / len + p.height(d))
        })
        .reduce(f64::min)
}

#[test]
fn the_swept_envelope_is_exact_along_a_line() {
    for tool in [flat(6.0), ball(6.0), bull(6.0, 1.0), drill(6.0)] {
        let p = Profile::of(&tool).unwrap();
        for (len, dz) in [(10.0, 0.0), (10.0, -2.0), (10.0, 3.0), (0.5, -4.0), (4.0, 20.0)] {
            for k in 0..60 {
                let u = -4.0 + 18.0 * ((k * 37) % 60) as f64 / 60.0;
                let v = 3.2 * ((k * 11) % 60) as f64 / 60.0;
                let exact = p.along_line(u, v, len, 1.0, dz);
                let brute = brute_line(&p, u, v, len, 1.0, dz);
                match (exact, brute) {
                    (Some(e), Some(b)) => {
                        // Never above the tool (the sweep removes all it should), and no lower.
                        assert!(e <= b + 1e-9, "{:?} u {u} v {v} len {len} dz {dz}: {e} above {b}", tool.kind);
                        assert!(b - e < 2e-4, "{:?} u {u} v {v} len {len} dz {dz}: {e} below {b}", tool.kind);
                    }
                    (None, None) => {}
                    // At the rim the brute force may just miss or just catch the cell.
                    (e, b) => {
                        assert!((u.hypot(v) - 3.0).abs() < 1e-3 || ((u - len).hypot(v) - 3.0).abs() < 1e-3 || (v - 3.0).abs() < 1e-3, "{e:?} {b:?}")
                    }
                }
            }
        }
    }
}

#[test]
fn a_pocket_has_its_floor_and_walls_exactly() {
    // A 20 × 20 mm pocket, 3 deep, with a 6 mm flat end mill: round the walls 3 mm in from
    // them, then across the middle.
    let mut moves = String::from("G0 X13. Y13.\nG0 Z1.\nG1 Z-3. F200\nG1 X27. F800\nY27.\nX13.\nY13.\n");
    let mut y = 13.0;
    while y < 27.0 {
        moves.push_str(&format!("G1 Y{y:.3}\nX27.\nG1 Y{:.3}\nX13.\n", y + 2.0));
        y += 4.0;
    }
    moves.push_str("G0 Z10.\n");
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let r = run(&program(&moves), &flat(6.0), &part, 0.1);
    assert_eq!(r.cells, 400 * 400);
    // Beyond the tool's reach in the corners.
    let in_corner = |x: f64, y: f64| {
        let (cx, cy) = (x.clamp(13.0, 27.0), y.clamp(13.0, 27.0));
        (x - cx).hypot(y - cy) > 3.0
    };
    for (x, y, h) in cells(&r) {
        let inside = (10.0..=30.0).contains(&x) && (10.0..=30.0).contains(&y);
        if inside && !in_corner(x, y) {
            assert_eq!(h, -3.0, "floor at {x} {y}");
        } else if !inside {
            assert_eq!(h, 0.0, "outside at {x} {y}");
        }
    }
    // The walls: the last cell in and the first out.
    assert_eq!(height_at(&r, 10.05, 20.0), -3.0);
    assert_eq!(height_at(&r, 9.95, 20.0), 0.0);
    assert_eq!(height_at(&r, 20.0, 29.95), -3.0);
    assert_eq!(height_at(&r, 20.0, 30.05), 0.0);
    // The corners are round, the tool's radius.
    assert_eq!(height_at(&r, 10.25, 10.25), 0.0);
    close(r.removed_volume, (400.0 - (4.0 - std::f64::consts::PI) * 9.0) * 3.0, 0.5);
    assert_eq!(r.gouge_cells, 0, "{r:?}");
    assert!(r.rapids_in_material.is_empty());
    assert!(r.leftover_max < 1e-9);
    // The first pass round is a slot, the passes across the middle are not.
    assert!(r.removals.iter().any(|m| m.slot && m.line == 11), "{:?}", r.removals);
    assert!(!r.warnings.is_empty());
}

/// Simulates `text` with a 6 mm flat end mill from the stock an earlier setup left: the stock's
/// box lowered to `start`, its material ending below at `under`, and held up to `part`, as a
/// next setup starts.
fn from_earlier(text: &str, start: &HeightMap, under: Option<&HeightMap>, part: &Surface, resolution: f64) -> SimReport {
    let plan = plan_sim(text, &Machine::three_axis(), &[flat(6.0)], &stock(), &params(0.01, resolution)).unwrap();
    let mut state = SimState::new(&plan);
    state.lower_to(start);
    if let Some(u) = under {
        state.raise_under(u);
    }
    state.hold_part(&plan.part_floor(part));
    state.advance(&plan, plan.moves(), 0.0).unwrap();
    state.finish(&plan, part)
}

/// A hole an earlier setup, upside down from this one, cut up to Z-5 from what is now the
/// stock's underside, bored from this side down past that: the column is empty, and the report
/// says so (its height is the stock's bottom), so what stands in the hole reads as nothing. (It
/// read the bore's bottom as material standing down to the stock's bottom.)
#[test]
fn a_column_cut_from_both_sides_reads_empty() {
    let start = HeightMap { x0: 0.0, y0: 0.0, dx: 0.5, dy: 0.5, nx: 80, ny: 80, heights: vec![0.0; 6400] };
    // Under: the stock's bottom, but for a round cut 10 mm across at X20 Y20 up to Z-5.
    let mut low = vec![-10.0; 6400];
    for j in 0..80 {
        for i in 0..80 {
            let (x, y) = (0.25 + 0.5 * i as f64, 0.25 + 0.5 * j as f64);
            if (x - 20.0).hypot(y - 20.0) <= 5.0 {
                low[j * 80 + i] = -5.0;
            }
        }
    }
    let under = HeightMap { x0: 0.0, y0: 0.0, dx: 0.5, dy: 0.5, nx: 80, ny: 80, heights: low };
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -9.0));
    let plunge = program(
        "G0 X20. Y20.
G0 Z1.
G1 Z-6. F200
G0 Z5.",
    );
    let r = from_earlier(&plunge, &start, Some(&under), &part, 0.5);
    let (h, _) = r.stock.highest_in_circle(Vec2::new(20.0, 20.0), 2.0).unwrap();
    assert!(h <= -10.0 + 1e-9, "the column under the plunge is empty: {h}");
    // Beside it, off the cut from below, the plunge's bottom is material down to the bottom.
    let plunge = program(
        "G0 X10. Y10.
G0 Z1.
G1 Z-6. F200
G0 Z5.",
    );
    let r = from_earlier(&plunge, &start, Some(&under), &part, 0.5);
    let (h, _) = r.stock.highest_in_circle(Vec2::new(10.0, 10.0), 2.0).unwrap();
    assert!((h + 6.0).abs() < 1e-6, "{h}");
}

/// The stock an earlier setup left, read a hair inside the part (a map on cells of its own),
/// starts held up to the part: a program that never touches the part cuts nothing into it, and
/// one that does is still measured against the part.
#[test]
fn a_stock_left_a_hair_inside_the_part_starts_held_up_to_it() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let start = HeightMap { x0: 0.0, y0: 0.0, dx: 0.5, dy: 0.5, nx: 80, ny: 80, heights: vec![-3.03; 6400] };
    let air = program(
        "G0 X20. Y20.
G0 Z5.",
    );
    let r = from_earlier(&air, &start, None, &part, 0.1);
    assert_eq!((r.gouge_cells, r.gouge_worst), (0, 0.0));
    // Not held up, the map's 0.03 mm inside the part reads as a cut into it.
    let plan = plan_sim(&air, &Machine::three_axis(), &[flat(6.0)], &stock(), &params(0.01, 0.1)).unwrap();
    let mut state = SimState::new(&plan);
    state.lower_to(&start);
    state.advance(&plan, plan.moves(), 0.0).unwrap();
    let r = state.finish(&plan, &part);
    assert!(r.gouge_cells > 0 && (r.gouge_worst - 0.03).abs() < 0.005, "{} cells, {} mm", r.gouge_cells, r.gouge_worst);
    // A plunge 1 mm into the part's top is a gouge of 1 mm.
    let plunge = program(
        "G0 X20. Y20.
G0 Z1.
G1 Z-4. F200
G0 Z5.",
    );
    let r = from_earlier(&plunge, &start, None, &part, 0.1);
    assert!(r.gouge_cells > 0 && (r.gouge_worst - 1.0).abs() < 0.05, "{} cells, {} mm", r.gouge_cells, r.gouge_worst);
}

#[test]
fn a_drill_stopped_at_the_stocks_bottom_leaves_its_points_cone_in_the_hole() {
    // A 6 mm drill (118°, point 1.8 mm) through the 10 mm stock, its tip at the stock's bottom:
    // the cone is left round the hole's foot. Its full diameter 0.5 mm below the bottom clears
    // the hole.
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, 0.0));
    let point = 3.0 / (59.0f64).to_radians().tan();
    let short = run(
        &program(
            "G0 X20. Y20.
G0 Z1.
G1 Z-10. F200
G0 Z10.",
        ),
        &drill(6.0),
        &part,
        0.05,
    );
    let (h, at) = short.stock.highest_in_circle(Vec2::new(20.0, 20.0), 2.9).unwrap();
    assert!(h > -10.0 + 1.0 && h < -10.0 + point, "the cone stands {h} at {at:?}");
    assert!((at.x - 20.0).hypot(at.y - 20.0) > 2.5, "highest at the wall: {at:?}");
    let through = format!(
        "G0 X20. Y20.
G0 Z1.
G1 Z{:.4} F200
G0 Z10.",
        -10.5 - point
    );
    let r = run(&program(&through), &drill(6.0), &part, 0.05);
    let (h, _) = r.stock.highest_in_circle(Vec2::new(20.0, 20.0), 2.9).unwrap();
    assert_eq!(h, -10.0, "cut to the stock's bottom all across");
    // Outside the hole the stock is whole; nothing in a circle no cell centre falls in.
    assert_eq!(r.stock.highest_in_circle(Vec2::new(30.0, 30.0), 1.0).map(|x| x.0), Some(0.0));
    assert!(r.stock.highest_in_circle(Vec2::new(20.0, 20.0), 0.01).is_none());
    assert!(r.stock.highest_in_circle(Vec2::new(-50.0, 20.0), 3.0).is_none());
    assert!(r.stock.highest_in_circle(Vec2::new(f64::NAN, 20.0), 3.0).is_none());
    assert!(r.stock.highest_in_circle(Vec2::new(20.0, 20.0), -1.0).is_none());
}

#[test]
fn a_ball_and_a_bull_nose_leave_their_own_shape() {
    for (tool, corner) in [(ball(6.0), 3.0f64), (bull(6.0, 1.0), 1.0)] {
        let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -4.0));
        let r = run(&program("G0 X5. Y20.\nG0 Z1.\nG1 Z-3. F200\nG1 X35. F800\nG0 Z10."), &tool, &part, 0.1);
        let flat_r: f64 = 3.0 - corner;
        for v in [0.05, 0.95, 1.55, 2.05, 2.55, 2.95] {
            let e = (v - flat_r).max(0.0);
            let want = -3.0 + corner - (corner * corner - e * e).max(0.0).sqrt();
            close(height_at(&r, 20.0, 20.0 + v), want, 1e-9);
            close(height_at(&r, 20.0, 20.0 - v), want, 1e-9);
        }
        assert_eq!(height_at(&r, 20.0, 23.05), 0.0);
    }
}

#[test]
fn level_and_helical_arcs_are_swept() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    // A half circle (counter-clockwise, over the top) of radius 8 about (20, 20) at Z-2.
    let r = run(&program("G0 X28. Y20.\nG0 Z1.\nG1 Z-2. F200\nG3 X12. Y20. I-8. J0. F800\nG0 Z10."), &flat(4.0), &part, 0.1);
    assert_eq!(height_at(&r, 20.0, 29.95), -2.0);
    assert_eq!(height_at(&r, 20.0, 30.05), 0.0);
    assert_eq!(height_at(&r, 20.0, 26.05), -2.0);
    assert_eq!(height_at(&r, 20.0, 25.95), 0.0);
    // Below the centre only the ends' round reaches.
    assert_eq!(height_at(&r, 20.0, 12.05), 0.0);
    // A helix down a full turn: never deeper than its end, the end's depth reached.
    let r = run(&program("G0 X25. Y20.\nG0 Z0.\nG2 X25. Y20.001 Z-4. I-5. J0. F500\nG0 Z10."), &ball(4.0), &part, 0.1);
    let lowest = cells(&r).into_iter().map(|c| c.2).fold(f64::INFINITY, f64::min);
    close(lowest, -4.0, 0.01);
    assert_eq!(r.gouge_cells, 0);
}

#[test]
fn a_gouging_program_is_caught() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let skim = run(&program("G0 X5. Y20.\nG0 Z1.\nG1 Z-5. F200\nG1 X35. F800\nG0 Z10."), &flat(6.0), &part, 0.1);
    assert_eq!(skim.gouge_cells, 0, "{skim:?}");
    let deep = run(&program("G0 X5. Y20.\nG0 Z1.\nG1 Z-6. F200\nG1 X35. F800\nG0 Z10."), &flat(6.0), &part, 0.1);
    assert!(deep.gouge_cells > 1000, "{}", deep.gouge_cells);
    close(deep.gouge_worst, 1.0, 1e-6);
    // Beside a wall within the tolerance is not a gouge: a step part, cut 0.005 into its wall.
    let step = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(20.0, 40.0, 0.0));
    let beside = run(&program("G0 X23.005 Y-5.\nG0 Z-1.\nG1 Y45. F800\nG0 Z10."), &flat(6.0), &step, 0.1);
    assert_eq!(beside.gouge_cells, 0, "{}", beside.gouge_worst);
    let into = run(&program("G0 X22.5 Y-5.\nG0 Z-1.\nG1 Y45. F800\nG0 Z10."), &flat(6.0), &step, 0.1);
    assert!(into.gouge_cells > 0);
    close(into.gouge_worst, 0.5, 0.06);
}

/// Watching the moves as they cut finds those that cut into the part, each with its depth
/// measured as the report measures a gouge.
#[test]
fn the_moves_that_cut_into_the_part_are_found_as_they_cut() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    // Lines 12 and 13: down a millimetre into the part, and back along it there.
    let text = program(
        "G0 X5. Y20.
G0 Z1.
G1 Z-5. F200
G1 X35. F800
G1 Z-6.
G1 X5.
G0 Z10.",
    );
    let (r, cuts) = simulate_watched(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), &part, &params(0.01, 0.1)).unwrap();
    close(r.gouge_worst, 1.0, 1e-6);
    assert_eq!(cuts.iter().map(|c| c.0).collect::<Vec<_>>(), vec![12, 13], "{cuts:?}");
    assert!(cuts.iter().all(|c| (c.1 - 1.0).abs() < 0.02), "{cuts:?}");
    // Watching changes nothing in the report; a program that stays on the part's top cuts nothing
    // into it.
    assert_eq!(simulate(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), &part, &params(0.01, 0.1)).unwrap(), r);
    let skim = program(
        "G0 X5. Y20.
G0 Z1.
G1 Z-5. F200
G1 X35. F800
G0 Z10.",
    );
    let (_, cuts) = simulate_watched(&skim, &Machine::three_axis(), &[flat(6.0)], &stock(), &part, &params(0.01, 0.1)).unwrap();
    assert!(cuts.is_empty(), "{cuts:?}");
}

#[test]
fn a_rapid_through_the_stock_is_caught() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let r = run(&program("G0 X5. Y20.\nG0 Z-2.\nG0 X35.\nG0 Z10."), &flat(6.0), &part, 0.2);
    let lines: Vec<usize> = r.rapids_in_material.iter().map(|m| m.line).collect();
    assert_eq!(lines, vec![9, 10], "{:?}", r.rapids_in_material);
    assert!(r.rapids_in_material.iter().all(|m| m.volume > 1.0));
    // A rapid down to just above the stock is not.
    let r = run(&program("G0 X5. Y20.\nG0 Z0.005\nG1 Z-2. F200\nG1 X35.\nG0 Z10."), &flat(6.0), &part, 0.2);
    assert!(r.rapids_in_material.is_empty(), "{:?}", r.rapids_in_material);
    // After a return home the tool comes down from above, not from where it was.
    let r = run(&program("G0 X5. Y20.\nG1 Z-2. F200\nG1 X10.\nG28 G91 Z0.\nG90\nG0 X35. Y35.\nG0 Z5.\nG1 Z-1.\nG0 Z10."), &flat(6.0), &part, 0.2);
    assert!(r.rapids_in_material.is_empty(), "{:?}", r.rapids_in_material);
    let r = run(&program("G0 X5. Y20.\nG1 Z-2. F200\nG28 G91 Z0.\nG90\nG0 X35. Y35.\nG0 Z-1.\nG0 Z10."), &flat(6.0), &part, 0.2);
    assert_eq!(r.rapids_in_material.iter().map(|m| m.line).collect::<Vec<_>>(), vec![13]);
}

#[test]
fn tools_are_taken_from_the_tool_changes() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let text = "G21\nT2 M6\nG0 X10. Y10. Z5.\nG1 Z-1. F100\nG0 Z5.\nT7 M6\nG0 X30. Y30. Z5.\nG1 Z-1. F100\nG0 Z10.\n";
    let tools = [Tool { number: 2, ..flat(2.0) }, Tool { number: 7, ..flat(10.0) }];
    let r = simulate(text, &Machine::three_axis(), &tools, &stock(), &part, &params(0.01, 0.1)).unwrap();
    assert_eq!(height_at(&r, 10.0, 10.95), -1.0);
    assert_eq!(height_at(&r, 10.0, 11.05), 0.0);
    assert_eq!(height_at(&r, 30.0, 34.95), -1.0);
    // A tool the program loads but the job lacks, and a move before any tool with two tools.
    assert!(simulate(text, &Machine::three_axis(), &tools[..1], &stock(), &part, &params(0.01, 0.1)).is_err());
    assert!(simulate("G0 X1 Y1 Z1\n", &Machine::three_axis(), &tools, &stock(), &part, &params(0.01, 0.1)).is_err());
}

#[test]
fn derived_stock_and_hostile_input() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let StockShape::Box { min, max } = stock_around(&part, &Machine::three_axis()).unwrap();
    close3(min, Vec3::new(0.0, 0.0, -10.0), 1e-12);
    close3(max, Vec3::new(40.0, 40.0, -5.0), 1e-12);
    let m = Machine::three_axis();
    let text = program("G1 X10. F100");
    let t = [flat(6.0)];
    // Too many cells, bad tolerances and resolutions, a stock without extent, a machine with a
    // rotary axis.
    assert!(matches!(simulate(&text, &m, &t, &stock(), &part, &params(0.01, 0.001)), Err(CamError::TooLarge(_))));
    for p in [params(0.0, 0.1), params(f64::NAN, 0.1), params(0.01, 0.0), params(0.01, f64::INFINITY), params(2.0, 0.1)] {
        assert!(simulate(&text, &m, &t, &stock(), &part, &p).is_err(), "{p:?}");
    }
    let flat_box = StockShape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(0.0, 10.0, 1.0) };
    assert!(simulate(&text, &m, &t, &flat_box, &part, &params(0.01, 0.1)).is_err());
    let nan_box = StockShape::Box { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::new(1.0, 10.0, 1.0) };
    assert!(simulate(&text, &m, &t, &nan_box, &part, &params(0.01, 0.1)).is_err());
    assert!(simulate(&text, &Machine::four_axis_a(), &t, &stock(), &part, &params(0.01, 0.1)).is_err());
    assert!(simulate(&text, &Machine::table_table_ac(), &t, &stock(), &part, &params(0.01, 0.1)).is_err());
    assert!(simulate(&text, &m, &[], &stock(), &part, &params(0.01, 0.1)).is_err());
    assert!(simulate(&text, &m, &[flat(6.0), flat(4.0)], &stock(), &part, &params(0.01, 0.1)).is_err());
    assert!(simulate(&text, &m, &[Tool { diameter: -1.0, ..flat(6.0) }], &stock(), &part, &params(0.01, 0.1)).is_err());
    // Far-off and enormous moves end quickly, as errors or as reports.
    for moves in [
        "G0 X999999999. Y-999999999. Z-5.\nG1 X-999999999. Y999999999. F100",
        "G1 X20. Y20. Z-1. F100\nG2 X20. Y20.001 Z-2. I-999999. J0.",
        "G1 X20. Y20. Z-1. F100\nG3 X21. Y20. I999999999. J5.",
        "G1 X20. Y20. Z-1. F100\nG1 X20.0000001 Z1000000. F100",
        "G1 X20. Y20. Z-1. F100\nG18 G2 X20. Z-1. I999999. K0.",
        "G1 X20. Y20. Z-1. F100\nG19 G3 Y20. Z-1. J0. K0.\nG17",
        "G1 X20. Y20. Z-1. F100\nG18 G2 X-999999999. Z5. I0.0000001 K999999999.",
        "T9.5 M6",
        "M6",
    ] {
        let _ = simulate(&program(moves), &m, &t, &stock(), &part, &params(0.01, 0.2));
    }
}

#[test]
fn a_fifty_millimetre_part_at_a_twentieth_of_a_millimetre() {
    // A dome over a 50 × 50 mm stock, finished with parallel passes of a 6 mm ball.
    let part = sphere(Vec3::new(25.0, 25.0, -20.0), 30.0, 96);
    let tool = ball(6.0);
    let heights = Heights { clearance: 20.0, retract: 12.0, top: 10.0, bottom: 0.0 };
    let path = parallel(
        &part,
        &tool,
        &ParallelParams {
            heights,
            feeds: Feeds { cut: 1500.0, plunge: 300.0 },
            descent_feed: None,
            stepover: 0.5,
            angle: 0.0,
            boundary: Boundary { rect: Some([Vec2::new(0.0, 0.0), Vec2::new(50.0, 50.0)]), region: None },
            stock_to_leave: 0.0,
            step: 1.0,
            tolerance: 0.01,
            zigzag: true,
            descent_limit: DEFAULT_DESCENT_LIMIT,
        },
    )
    .unwrap();
    let program = Program5 {
        name: "dome".into(),
        operations: vec![Operation5 {
            name: "finish".into(),
            tool: tool.clone(),
            spindle_rpm: 18000.0,
            coolant: false,
            path: Toolpath5::from_3axis(&path, 0.001).unwrap(),
            plunge_feed: None,
        }],
        work_offset: None,
    };
    let machine = Machine::three_axis();
    let post = Post5 { base: Post::rs274(), ..Post5::linuxcnc_4axis() };
    let text = emit5(&program, &post, &machine).unwrap();
    let stock = StockShape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(50.0, 50.0, 10.0 + 0.3) };
    let started = Instant::now();
    let r = simulate(&text, &machine, std::slice::from_ref(&tool), &stock, &part, &params(0.01, 0.05)).unwrap();
    let secs = started.elapsed().as_secs_f64();
    eprintln!(
        "50 x 50 mm at 0.05 mm: {} cells, {} lines, {:.2} s; gouge {} cells, leftover max {:.4} mean {:.4}, {:.0} mm³ removed",
        r.cells,
        text.lines().count(),
        secs,
        r.gouge_cells,
        r.leftover_max,
        r.leftover_mean,
        r.removed_volume
    );
    assert_eq!(r.cells, 1000 * 1000);
    assert_eq!(r.gouge_cells, 0, "{}", r.gouge_worst);
    assert!(r.rapids_in_material.is_empty());
    let visits = crate::sim::SIM_VISITS.with(std::cell::Cell::get);
    // Some 233 updates a cell (2.3e8), half as much again allowed.
    assert!(visits <= 350_000_000, "{visits} cell updates ({secs} s)");
}

#[test]
fn an_arc_back_to_its_start_is_a_full_circle() {
    // The controller runs an arc whose end is its start as a whole turn, level or helical.
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let level = run(&program("G0 X25. Y20.\nG0 Z1.\nG1 Z-2. F200\nG2 X25. Y20. I-5. J0. F800\nG0 Z10."), &flat(4.0), &part, 0.1);
    for (x, y) in [(15.05, 20.0), (20.0, 24.95), (20.0, 15.05), (24.95, 20.0)] {
        assert_eq!(height_at(&level, x, y), -2.0, "the ring at {x} {y}");
    }
    assert_eq!(height_at(&level, 20.05, 20.05), 0.0, "the middle is left");
    close(level.removed_volume, std::f64::consts::PI * (7.0f64.powi(2) - 3.0f64.powi(2)) * 2.0, 1.0);
    // Its time is a whole turn's: 2π·5 mm at 800 mm/min, and the plunge.
    assert!(level.minutes > std::f64::consts::TAU * 5.0 / 800.0, "{}", level.minutes);
    // Counter-clockwise, and down a full turn of a helix: half way round is half way down.
    let helix = run(&program("G0 X25. Y20.\nG0 Z0.\nG3 X25. Y20. Z-4. I-5. J0. F500\nG0 Z10."), &ball(4.0), &part, 0.1);
    close(height_at(&helix, 15.05, 20.0), -2.0, 0.05);
    let lowest = cells(&helix).into_iter().map(|c| c.2).fold(f64::INFINITY, f64::min);
    close(lowest, -4.0, 0.01);
    // A full circle that goes too deep is a gouge (the part is at -5).
    let deep = run(&program("G0 X25. Y20.\nG0 Z1.\nG1 Z-4. F200\nG2 X25. Y20. Z-6. I-5. J0. F800\nG0 Z10."), &flat(4.0), &part, 0.1);
    assert!(deep.gouge_cells > 100, "{}", deep.gouge_cells);
    close(deep.gouge_worst, 1.0, 0.05);
}

#[test]
fn arcs_in_the_xz_and_yz_planes_turn_about_their_own_centres() {
    // Half circles of radius 3 from (10, 20, 0): in G18 a G2 (clockwise seen from +Y) dips to
    // Z-3 under X13, a G3 rises over it; in G19 a G3 (counter-clockwise seen from +X) dips to
    // Z-3 under Y23. A 2 mm ball, so the trench's bottom is the tip's lowest point.
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let tool = ball(2.0);
    let dip = run(&program("G0 X10. Y20.\nG0 Z1.\nG1 Z0. F200\nG18 G2 X16. Z0. I3. K0. F500\nG17\nG0 Z10."), &tool, &part, 0.1);
    close(height_at(&dip, 13.0, 20.0), -3.0, 0.01);
    // Part way round, the ball's envelope: a torus of tube radius 1 about the circle of its
    // centres (radius 3 about X13 Z1), at the cell's centre (10.85, 20.05).
    let rho = 3.0 + (1.0f64 - 0.05 * 0.05).sqrt();
    close(height_at(&dip, 10.85, 20.0), 1.0 - (rho * rho - 2.15 * 2.15).sqrt(), 0.01);
    assert_eq!(height_at(&dip, 13.0, 21.25), 0.0, "the trench is the ball's width");
    assert_eq!(dip.gouge_cells, 0);
    let over = run(&program("G0 X10. Y20.\nG0 Z1.\nG1 Z0. F200\nG18 G3 X16. Z0. I3. K0. F500\nG17\nG0 Z10."), &tool, &part, 0.1);
    assert_eq!(over.removed_volume, 0.0, "over the top cuts nothing");
    let side = run(&program("G0 X10. Y20.\nG0 Z1.\nG1 Z0. F200\nG19 G3 Y26. Z0. J3. K0. F500\nG17\nG0 Z10."), &tool, &part, 0.1);
    close(height_at(&side, 10.0, 23.0), -3.0, 0.01);
    assert_eq!(height_at(&side, 13.0, 23.0), 0.0);
    // Its time is the half circle's: π·3 mm at 500 mm/min, the plunge at 200.
    close(side.minutes, std::f64::consts::PI * 3.0 / 500.0 + 1.0 / 200.0, 1e-6);
    // Too deep a dip is a gouge.
    let deep = run(&program("G0 X10. Y20.\nG0 Z1.\nG1 Z-3. F200\nG18 G2 X16. Z-3. I3. K0. F500\nG17\nG0 Z10."), &tool, &part, 0.1);
    close(deep.gouge_worst, 1.0, 0.02);
}

#[test]
fn a_long_three_axis_move_is_not_refused_for_a_fine_tolerance() {
    // Tool positions are only placed along the blocks on a 4th axis: a 3-axis program with a
    // drill and a fine tolerance runs whatever its moves' lengths.
    let stock = StockShape::Box { min: Vec3::new(0.0, 0.0, -10.0), max: Vec3::new(1500.0, 40.0, 0.0) };
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(1500.0, 40.0, -5.0));
    let text = program("G0 X10. Y20. Z5.\nG1 Z-1. F100\nG0 Z5.\nG0 X1400.\nG1 Z-1. F100\nG0 Z10.");
    let r = simulate(&text, &Machine::three_axis(), &[drill(6.0)], &stock, &part, &params(0.0001, 1.0)).unwrap();
    assert!(r.rapids_in_material.is_empty(), "{:?}", r.rapids_in_material);
    assert_eq!(r.gouge_cells, 0);
    assert!(r.removed_volume > 0.0);
}

/// Every kind of tool has a shape in the simulation: a chamfer mill is its flat tip then its
/// cone, a face mill a flat or bull nose by its inserts' corners, a thread mill its whole
/// radius (as the gouge check takes it) and a drill its own point angle.
#[test]
fn the_simulation_shapes_every_kind_of_tool() {
    let mut chamfer = Tool::new(7, ToolKind::Chamfer, 10.0, 10.0);
    chamfer.angle_deg = Some(90.0);
    chamfer.tip_diameter = Some(1.0);
    let p = Profile::of(&chamfer).unwrap();
    assert!((p.radius - 5.0).abs() < 1e-12);
    assert!(p.height(0.3).abs() < 1e-12, "the flat tip");
    assert!((p.height(3.0) - 2.5).abs() < 1e-9, "45 degrees beyond the tip: {}", p.height(3.0));
    assert!((p.height(5.0) - 4.5).abs() < 1e-9);
    let mut face = Tool::new(1, ToolKind::FaceMill, 20.0, 6.0);
    assert!(Profile::of(&face).unwrap().height(9.9).abs() < 1e-12, "square inserts cut flat");
    face.corner_radius = Some(1.0);
    let p = Profile::of(&face).unwrap();
    assert!(p.height(8.9).abs() < 1e-12 && (p.height(10.0) - 1.0).abs() < 1e-9, "rounded inserts");
    let thread = Profile::of(&Tool::new(5, ToolKind::ThreadMill, 6.0, 12.0)).unwrap();
    assert!(thread.height(2.9).abs() < 1e-12 && (thread.radius - 3.0).abs() < 1e-12);
    let mut drill = Tool::new(3, ToolKind::Drill, 6.0, 30.0);
    drill.point_angle_deg = Some(90.0);
    assert!((Profile::of(&drill).unwrap().height(2.0) - 2.0).abs() < 1e-9, "a 90 degree point");
}

/// A small random number generator for chunk sizes.
fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

/// `simulate` as its parts: the plan, a state advanced a random number of steps at a time (and,
/// with `fractions`, to random places inside steps), and the finish.
fn chunked(
    text: &str,
    machine: &Machine,
    tools: &[Tool],
    stock: &StockShape,
    part: &Surface,
    params: &SimParams,
    seed: u64,
    fractions: bool,
) -> SimReport {
    let plan = plan_sim(text, machine, tools, stock, params).unwrap();
    let mut state = SimState::new(&plan);
    let mut seed = seed;
    while state.at().0 < plan.moves() {
        let (k, f) = state.at();
        let to = k + (lcg(&mut seed) % 4) as usize + usize::from(!fractions);
        let frac = if fractions { (lcg(&mut seed) % 1000) as f64 / 1000.0 } else { 0.0 };
        let frac = if to == k { frac.max(f + 0.001).min(1.0) } else { frac };
        state.advance(&plan, to, frac).unwrap();
        assert!(state.advance(&plan, 0, 0.0).is_err() || state.at() == (0, 0.0), "cutting back is refused");
    }
    state.finish(&plan, part)
}

fn pocket_program() -> String {
    let mut moves = String::from(
        "G0 X13. Y13.
G0 Z1.
G1 Z-3. F200
G1 X27. F800
Y27.
X13.
Y13.
",
    );
    let mut y = 13.0;
    while y < 27.0 {
        moves.push_str(&format!(
            "G1 Y{y:.3}
X27.
G1 Y{:.3}
X13.
",
            y + 2.0
        ));
        y += 4.0;
    }
    moves.push_str(
        "G2 X20. Y20. I3.5 J3.5
G0 Z10.
",
    );
    program(&moves)
}

#[test]
fn advancing_a_part_at_a_time_leaves_what_one_run_does() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let three = Machine::three_axis();
    let cases: Vec<(String, Tool)> = vec![
        (pocket_program(), flat(6.0)),
        (
            program(
                "G0 X5. Y20.
G0 Z1.
G1 Z-3. F200
G1 X35. F800
G3 X5. Y20. I-15. J0.
G0 Z10.",
            ),
            ball(6.0),
        ),
        (
            program(
                "G0 X25. Y20.
G0 Z0.
G2 X25. Y20.001 Z-4. I-5. J0. F500
G18 G2 X35. Z-4. I5. K0.
G17
G0 Z10.",
            ),
            bull(6.0, 1.0),
        ),
    ];
    for (text, tool) in &cases {
        let p = params(0.01, 0.2);
        let whole = simulate(text, &three, std::slice::from_ref(tool), &stock(), &part, &p).unwrap();
        for seed in [1, 7, 42] {
            // Whole steps: bit for bit.
            assert_eq!(chunked(text, &three, std::slice::from_ref(tool), &stock(), &part, &p, seed, false), whole);
            // Parts of steps: the same stock, to rounding.
            let parts = chunked(text, &three, std::slice::from_ref(tool), &stock(), &part, &p, seed, true);
            let (StockGrid::Heights { heights: a, .. }, StockGrid::Heights { heights: b, .. }) = (&parts.stock, &whole.stock);
            assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9), "seed {seed}");
            close(parts.removed_volume, whole.removed_volume, 1e-6 * whole.removed_volume.max(1.0));
        }
    }
}

#[test]
fn tile_maxima_bound_their_cells() {
    let plan = plan_sim(&pocket_program(), &Machine::three_axis(), &[flat(6.0)], &stock(), &params(0.01, 0.1)).unwrap();
    let mut state = SimState::new(&plan);
    let mut seed = 5u64;
    let (nx, ny) = (plan.grid().columns(), plan.grid().rows());
    for to in [3usize, 7, 12, plan.moves()] {
        state.advance(&plan, to, 0.0).unwrap();
        for _ in 0..200 {
            let (a, b) = ((lcg(&mut seed) as usize) % nx, (lcg(&mut seed) as usize) % nx);
            let (c, d) = ((lcg(&mut seed) as usize) % ny, (lcg(&mut seed) as usize) % ny);
            let ((i0, i1), (j0, j1)) = ((a.min(b), a.max(b)), (c.min(d), c.max(d)));
            // Exact over the whole tiles covering the cells asked.
            let mut brute = f64::NEG_INFINITY;
            for j in (j0 / TILE * TILE)..((j1 / TILE + 1) * TILE).min(ny) {
                for i in (i0 / TILE * TILE)..((i1 / TILE + 1) * TILE).min(nx) {
                    brute = brute.max(state.value(&plan, i, j).unwrap());
                }
            }
            assert_eq!(state.max_in(&plan, (i0, i1), (j0, j1)), brute);
        }
    }
    // Every tile the pocket reaches changed its version; a tile far from it did not.
    // The pocket is cells 100 to 300 each way (0.1 mm cells), tiles 6 to 18.
    assert!(state.tile_version(12, 12) > 0);
    assert_eq!(state.tile_version(0, 0), 0);
    assert!(state.last_tool(&plan, 200, 200) == 0 && state.last_tool(&plan, 0, 0) == crate::sim::UNCUT);
}

#[test]
fn the_shank_rubbing_and_the_spindle_are_seen() {
    // Two millimetres of flutes, a slot five deep in one pass: three stand above the flutes.
    let short = Tool::new(1, ToolKind::FlatEndmill, 6.0, 2.0);
    let text = program(
        "G0 X5. Y20.
G0 Z1.
G1 Z-5. F200
G1 X35. F800
M5
G1 X36.
G0 Z10.",
    );
    let plan = plan_sim(&text, &Machine::three_axis(), std::slice::from_ref(&short), &stock(), &params(0.01, 0.2)).unwrap();
    let mut state = SimState::new(&plan);
    state.advance(&plan, plan.moves(), 0.0).unwrap();
    let slot = (0..plan.moves()).find(|&k| plan.step(k).unwrap().line == 11).unwrap();
    close(state.removed()[slot].above, 3.0, 1e-9);
    close(state.removed()[slot].above_at.z, -3.0, 1e-9);
    // The spindle turns from `S12000 M3` and stops at `M5`.
    assert_eq!(plan.step(slot).unwrap().rpm, Some(12000.0));
    assert_eq!(plan.step(slot + 1).unwrap().rpm, Some(0.0));
    // A program that never says has no speed.
    let silent = plan_sim(
        "G21
G0 X0 Y0 Z5
G1 Z-1 F100
",
        &Machine::three_axis(),
        &[flat(2.0)],
        &stock(),
        &params(0.01, 0.5),
    )
    .unwrap();
    assert_eq!(silent.step(1).unwrap().rpm, None);
}

/// A gouge is measured over the material a cut column runs through: a column down the air of
/// a slot between two blocks is no gouge however far its middle is from their walls, and a
/// column through the side of a block grazing it, or into it, is as deep as it goes in.
#[test]
fn a_gouge_is_measured_over_the_material_a_column_cuts() {
    let left = block(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(-2.0, 10.0, 0.0));
    let right = block(Vec3::new(2.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 0.0));
    let floor = block(Vec3::new(-10.0, -10.0, -12.0), Vec3::new(10.0, 10.0, -10.0));
    let part = Surface::new(left.triangles().iter().chain(right.triangles()).chain(floor.triangles()).copied().collect()).unwrap();
    // Down the slot to its floor: air all the way, 2 mm from the walls at its middle.
    close(crate::sim::column_depth(&part, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -10.0), 20.0), 0.0, 1e-9);
    // Into the floor by half a millimetre.
    close(crate::sim::column_depth(&part, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -10.5), 20.0), 0.5, 1e-9);
    // Slanting in through the top of a block and out of its side into the slot: as deep as the
    // middle of the stretch inside it.
    let d = crate::sim::column_depth(&part, Vec3::new(-3.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -5.0), 20.0);
    assert!(d > 0.1 && d < 0.5, "{d}");
    // Ending inside a block: as deep as its end.
    close(crate::sim::column_depth(&part, Vec3::new(-6.0, 0.0, 5.0), Vec3::new(-6.0, 0.0, -3.0), 20.0), 3.0, 1e-9);
}

/// The gouge measure does not depend on which way the part's triangles face: a part whose
/// triangles all face inwards (an STL wound the other way), or merged bodies that overlap, or a
/// column that starts inside the part, still have their gouges measured, never passed as air.
#[test]
fn a_gouge_is_found_in_a_part_whose_triangles_face_inwards() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    let inside_out = Surface::new(part.triangles().iter().map(|t| [t[0], t[2], t[1]]).collect()).unwrap();
    // A column down into the part by a millimetre.
    let (from, to) = (Vec3::new(20.0, 20.0, 5.0), Vec3::new(20.0, 20.0, -6.0));
    close(crate::sim::column_depth(&part, from, to, 20.0), 1.0, 1e-9);
    let d = crate::sim::column_depth(&inside_out, from, to, 20.0);
    assert!(d > 1.0 - 1e-9, "{d}");
    // Two bodies overlapping, merged: the column ends inside both.
    let other = block(Vec3::new(10.0, 10.0, -12.0), Vec3::new(30.0, 30.0, -7.0));
    let merged = Surface::new(part.triangles().iter().chain(other.triangles()).copied().collect()).unwrap();
    let d = crate::sim::column_depth(&merged, from, Vec3::new(20.0, 20.0, -9.0), 20.0);
    assert!(d > 1.0 - 1e-9, "{d}");
    // A column that starts inside the part (a part reaching above where it starts).
    let d = crate::sim::column_depth(&part, Vec3::new(20.0, 20.0, -5.5), to, 20.0);
    assert!(d > 1.0 - 1e-9, "{d}");
    // The whole simulation: the deep pass gouges the inside-out part as it does the part.
    let text = program("G0 X5. Y20.\nG0 Z1.\nG1 Z-6. F200\nG1 X35. F800\nG0 Z10.");
    let deep = run(&text, &flat(6.0), &inside_out, 0.1);
    assert!(deep.gouge_cells > 1000, "{}", deep.gouge_cells);
    close(deep.gouge_worst, 1.0, 1e-6);
}

/// A sliver left along a steep wall (the finish 0.06 mm short of it, a little more than half a
/// cell) is as thin as it is: the cells beside the wall that kept their material are measured
/// in 3D from the part, not as the wall's full height. It was reported as 3 mm left on the
/// part, the slot's depth, which alarms an operator for a wall within a cell of its size.
#[test]
fn a_sliver_along_a_wall_is_reported_as_thin_as_it_is() {
    // A 40 × 40 block with a slot 20 wide (X 10 to 30) and 3 deep along Y, under stock 0.5 mm
    // above it.
    let mut tris = Vec::new();
    for (lo, hi) in [
        (Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0)),
        (Vec3::new(0.0, 0.0, -3.0), Vec3::new(10.0, 40.0, 0.0)),
        (Vec3::new(30.0, 0.0, -3.0), Vec3::new(40.0, 40.0, 0.0)),
    ] {
        tris.extend_from_slice(block(lo, hi).triangles());
    }
    let part = Surface::new(tris).unwrap();
    let stock = StockShape::Box { min: Vec3::new(0.0, 0.0, -10.0), max: Vec3::new(40.0, 40.0, 0.5) };
    let tool = flat(6.0);
    // Faced to the part's top, then the slot along Y, its walls left 0.06 mm thick.
    let mut moves = String::from("G0 X-5. Y-2. Z1.\nG1 Z0. F200\n");
    let mut y = -2.0;
    let mut left = true;
    while y <= 42.0 {
        moves.push_str(&format!("G1 X{} Y{y:.3} F800\n", if left { 45.0 } else { -5.0 }));
        left = !left;
        y += 4.0;
        moves.push_str(&format!("G1 Y{y:.3}\n"));
    }
    moves.push_str("G0 Z1.\n");
    for x in [13.06, 17.0, 21.0, 25.0, 26.94] {
        moves.push_str(&format!("G0 X{x:.3} Y-5.\nG1 Z-3. F200\nG1 Y45. F800\nG0 Z1.\n"));
    }
    let r = simulate(&program(&moves), &Machine::three_axis(), std::slice::from_ref(&tool), &stock, &part, &params(0.01, 0.1)).unwrap();
    assert_eq!(r.gouge_cells, 0, "{} at {:?}", r.gouge_worst, r.gouge_at);
    // The cells beside the walls (centres 0.05 mm from them) kept their material: as thin as
    // that, never the slot's depth.
    assert!(r.leftover_max > 0.03 && r.leftover_max < 0.1, "{} at {:?}", r.leftover_max, r.leftover_at);
    let at = r.leftover_at.unwrap();
    assert!((at.x - 10.05).abs() < 0.06 || (at.x - 29.95).abs() < 0.06, "{at:?}");
}

/// Stock left standing in a through hole is leftover: a column whose ray from above meets no
/// part (the hole goes all the way through) but that the part closes round on every side is
/// counted, from the part's bottom up. A plate 40 x 40 with a 10 x 10 hole through it, faced
/// to its top: the hole keeps 9 mm of stock.
#[test]
fn stock_left_in_a_through_hole_is_leftover() {
    let (lo, hi) = (-10.0, -1.0);
    let mut tris = Vec::new();
    for (a, b) in [((0.0, 0.0), (40.0, 15.0)), ((0.0, 25.0), (40.0, 40.0)), ((0.0, 15.0), (15.0, 25.0)), ((25.0, 15.0), (40.0, 25.0))] {
        tris.extend_from_slice(block(Vec3::new(a.0, a.1, lo), Vec3::new(b.0, b.1, hi)).triangles());
    }
    let part = Surface::new(tris).unwrap();
    let mut moves = String::from("G0 X-5. Y0.\nG1 Z-1. F300\n");
    let mut y = 0.0;
    while y <= 40.0 {
        moves += &format!("G1 Y{y} F600\nG1 X45.\nG0 Z5.\nG0 X-5.\nG1 Z-1.\n");
        y += 4.0;
    }
    moves.push_str("G0 Z10.\n");
    let r = run(&program(&moves), &flat(6.0), &part, 0.25);
    assert_eq!(r.gouge_cells, 0, "{}", r.gouge_worst);
    assert!(r.leftover_max > 4.0, "{} mm left at {:?}: the hole's stock is not counted", r.leftover_max, r.leftover_at);
    let at = r.leftover_at.unwrap();
    assert!((15.0..=25.0).contains(&at.x) && (15.0..=25.0).contains(&at.y), "{at:?}");
}
