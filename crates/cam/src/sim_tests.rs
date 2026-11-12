//! Tests of the stock-removal simulation: the tool's swept envelope against closed forms and
//! brute force, a pocket's floor and walls, gouges and rapids caught, a rotary finish, indexed
//! work on a bar, the side lines under an undercut, what is left over the whole part, the
//! limits, how long a 50 × 50 mm part takes, the simulation advanced a part at a time, the
//! stock and tool meshes, and the findings.

use std::time::Instant;

use solidcore_record::{Vec2, Vec3};

use crate::adaptive::HeightMap;
use crate::foundation_tests::*;
use crate::gcode5::{Post5, emit5};
use crate::kinematics::Machine;
use crate::sim::{
    Carried, Profile, SimParams, SimReport, SimState, StockGrid, StockShape, TILE, plan_sim, simulate, simulate_from, simulate_watched, stock_around,
};
use crate::*;
use solidcraft_geom::prelude::*;

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
    let StockGrid::Heights { x0, y0, dx, dy, nx, heights, .. } = &r.stock else { panic!("not a heightfield") };
    let (i, j) = (((x - x0) / dx).floor() as usize, ((y - y0) / dy).floor() as usize);
    heights[j * nx + i]
}

/// Every cell centre and its height.
fn cells(r: &SimReport) -> Vec<(f64, f64, f64)> {
    let StockGrid::Heights { x0, y0, dx, dy, nx, heights, .. } = &r.stock else { panic!("not a heightfield") };
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

/// The first point of the ray `(x, ρ s, ρ c)` inside the tool, by brute force.
fn brute_ray(p: &Profile, dx: f64, y: f64, z: f64, s: f64, c: f64, cap: f64) -> Option<f64> {
    let n = 400_000;
    (0..=n).map(|k| cap * k as f64 / n as f64).find(|&rho| {
        let d = dx.hypot(rho * s - y);
        d <= p.radius && rho * c >= z + p.height(d)
    })
}

#[test]
fn rays_enter_the_tool_where_brute_force_finds_it() {
    for tool in [flat(6.0), ball(6.0), bull(6.0, 1.5), drill(6.0)] {
        let p = Profile::of(&tool).unwrap();
        for k in 0..80 {
            let dx = -3.5 + 7.0 * ((k * 13) % 80) as f64 / 80.0;
            let y = -4.0 + 8.0 * ((k * 29) % 80) as f64 / 80.0;
            let z = 5.0 + 10.0 * ((k * 7) % 80) as f64 / 80.0;
            let phi = -0.6 + 1.2 * ((k * 53) % 80) as f64 / 80.0;
            let (s, c) = phi.sin_cos();
            let cap = 30.0;
            let exact = p.ray_entry(dx, y, z, s, c, cap);
            let brute = brute_ray(&p, dx, y, z, s, c, cap);
            match (exact, brute) {
                (Some(e), Some(b)) => {
                    assert!(e <= b + 1e-9, "{:?}: {e} after {b}", tool.kind);
                    assert!(b - e < 1e-3, "{:?}: {e} well before {b}", tool.kind);
                }
                (None, None) => {}
                (e, b) => {
                    // Only grazing rays may disagree.
                    let touch = e.or(b).unwrap_or(0.0);
                    let d = dx.hypot(touch * s - y);
                    assert!((d - 3.0).abs() < 1e-2 || (touch - cap).abs() < 1e-2, "{:?}: {e:?} against {b:?}", tool.kind);
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

/// The 20 × 20 mm pocket program of [`a_pocket_has_its_floor_and_walls_exactly`], 3 deep.
fn square_pocket_moves() -> String {
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
        "G0 Z10.
",
    );
    moves
}

/// A box's triangles, facing out.
fn box_triangles(lo: Vec3, hi: Vec3) -> Vec<[Vec3; 3]> {
    let p = |i: usize| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    quads.iter().flat_map(|q| [[p(q[0]), p(q[1]), p(q[2])], [p(q[0]), p(q[2]), p(q[3])]]).collect()
}

#[test]
fn the_whole_part_is_held_against_what_is_left_not_only_where_the_program_cut() {
    let text = program(&square_pocket_moves());
    // The part is the pocket's floor block: everything the program cut, nothing left on it.
    let done = block(Vec3::new(11.0, 11.0, -10.0), Vec3::new(29.0, 29.0, -3.0));
    let r = run(&text, &flat(6.0), &done, 0.1);
    assert_eq!(r.gouge_cells, 0);
    assert!(r.part_left.volume < 1e-9, "{:?}", r.part_left);
    assert_eq!(r.part_left.max(), 0.0);
    assert_eq!(r.part_left.hidden_area, 0.0);
    // The part is faced 3 mm down all over, but the program only cut the pocket: 3 mm left over
    // the rest of the top (where the program never cut, so the leftover where it cut is none).
    let faced = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let r = run(&text, &flat(6.0), &faced, 0.1);
    assert!(r.leftover_max < 1e-9, "{}", r.leftover_max);
    let left = &r.part_left;
    // 40² less the pocket (its corners are the tool's radius), 3 deep.
    let expected = (1600.0 - (400.0 - (4.0 - std::f64::consts::PI) * 9.0)) * 3.0;
    assert!((left.volume - expected).abs() < 0.03 * expected, "{} against {expected}", left.volume);
    assert_eq!(left.hidden_volume, 0.0);
    close(left.max(), 3.0, 1e-6);
    assert!(!left.spots.is_empty() && left.spots.len() <= crate::sim::SPOTS);
    assert!(left.spots.iter().all(|(t, at)| *t > 0.01 && !((10.0..=30.0).contains(&at.x) && (10.0..=30.0).contains(&at.y))), "{:?}", left.spots);
    // A part with material over air over material (a slot through it along X): faces from above
    // a height map cannot see, and the stock in the slot is left under the part's own material.
    let mut tris = box_triangles(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -6.0));
    tris.extend(box_triangles(Vec3::new(0.0, 0.0, -4.0), Vec3::new(40.0, 40.0, 0.0)));
    let slotted = Surface::new(tris).unwrap();
    let r = run(
        &program(
            "G0 Z10.
",
        ),
        &flat(6.0),
        &slotted,
        0.1,
    );
    close(r.part_left.hidden_area, 1600.0, 1.0);
    close(r.part_left.hidden_volume, 1600.0 * 2.0, 1.0);
    close(r.part_left.volume, r.part_left.hidden_volume, 1e-9);
    // The slot has part material over and under it along Z: neither this setup nor one turned
    // over reaches it.
    assert!(r.part_left.pieces.iter().all(|p| p.hidden && p.enclosed));
    assert!(!r.part_left.spots.is_empty());
    // An overhang with nothing under it along Z: what stands under it is hidden, but not
    // enclosed.
    let mut tris = box_triangles(Vec3::new(0.0, 0.0, -4.0), Vec3::new(40.0, 40.0, 0.0));
    tris.extend(box_triangles(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 10.0, -4.0)));
    let overhung = Surface::new(tris).unwrap();
    let r = run(
        &program(
            "G0 Z10.
",
        ),
        &flat(6.0),
        &overhung,
        0.1,
    );
    close(r.part_left.hidden_volume, 40.0 * 30.0 * 6.0, 0.01 * 7200.0);
    assert!(r.part_left.pieces.iter().all(|p| p.hidden && !p.enclosed));
    // Turned over, a later setup reaches what stands under the overhang.
    close(r.part_left.volume, r.part_left.hidden_volume, 1e-9);
}

#[test]
fn an_undercut_a_vertical_tool_cannot_reach_is_left_open_on_the_side_lines() {
    // A slab with a plate over half of it, 2 mm of air between (an undercut under the plate, open
    // at the plate's edge): a 6 mm end mill clears the open half down to the slab, and cannot
    // reach under the plate.
    let mut tris = box_triangles(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -6.0));
    tris.extend(box_triangles(Vec3::new(0.0, 0.0, -4.0), Vec3::new(40.0, 20.0, 0.0)));
    let part = Surface::new(tris).unwrap();
    let mut moves = String::from(
        "G0 X-5. Y23.
G0 Z1.
G1 Z-6. F200
",
    );
    let mut y = 23.0;
    let mut forth = true;
    while y <= 43.0 {
        moves.push_str(&format!(
            "G1 Y{y:.3} F800
G1 X{}
",
            if forth { "45." } else { "-5." }
        ));
        forth = !forth;
        y += 4.0;
    }
    moves.push_str(
        "G0 Z10.
",
    );
    let text = program(&moves);
    let machine = Machine::three_axis();
    let plan = plan_sim(&text, &machine, &[flat(6.0)], &stock(), &params(0.01, 0.1)).unwrap();
    let mut state = SimState::new(&plan);
    assert!(state.track_sides(&plan));
    state.advance(&plan, plan.moves(), 0.0).unwrap();
    // The side lines hold the cut: along X under the plate, all of it; along Y in the air under
    // the plate's height, up to the plate's edge.
    let line = state.side_line(0, Vec3::new(20.0, 10.0, -5.0)).unwrap();
    assert_eq!(line.len(), 1);
    close(line[0][0], 0.0, 1e-9);
    close(line[0][1], 40.0, 1e-9);
    let line = state.side_line(1, Vec3::new(20.0, 10.0, -5.0)).unwrap();
    assert_eq!(line.len(), 1);
    close(line[0][0], 0.0, 1e-9);
    close(line[0][1], 20.0, 0.01);
    let r = state.finish(&plan, &part);
    assert_eq!(r.gouge_cells, 0, "{:?}", r.gouge_at);
    // The undercut's material is left, held against the whole part, and no setup along Z cuts
    // it.
    let undercut = 40.0 * 20.0 * 2.0;
    close(r.part_left.volume, undercut, 0.05 * undercut);
    assert!(r.part_left.pieces.iter().all(|p| p.enclosed), "{:?}", r.part_left.pieces.first());
    let spot = r.part_left.spots.first().map(|s| s.1).unwrap();
    assert!(spot.y < 20.0 && spot.z > -6.0 && spot.z < -4.0, "{spot:?}");
    // `simulate` keeps the side lines for such a part by itself, and hands them on.
    let auto = simulate(&text, &machine, &[flat(6.0)], &stock(), &part, &params(0.01, 0.1)).unwrap();
    assert_eq!(auto.sides.len(), 2);
    assert_eq!(auto.gouge_cells, 0);
    close(auto.part_left.volume, r.part_left.volume, 1e-9);
}

#[test]
fn a_plain_pocket_simulates_the_same_with_or_without_side_lines() {
    let text = program(&square_pocket_moves());
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let machine = Machine::three_axis();
    let plan = plan_sim(&text, &machine, &[flat(6.0)], &stock(), &params(0.01, 0.1)).unwrap();
    let mut off = SimState::new(&plan);
    off.advance(&plan, plan.moves(), 0.0).unwrap();
    let mut on = SimState::new(&plan);
    assert!(on.track_sides(&plan));
    on.advance(&plan, plan.moves(), 0.0).unwrap();
    // The side lines cut as the height map does: along X through the pocket at mid depth, the
    // pocket (13 to 27 with the tool's 3 mm radius) is out of the line.
    let line = on.side_line(0, Vec3::new(20.0, 20.0, -1.5)).unwrap();
    assert_eq!(line.len(), 2, "{line:?}");
    close(line[0][1], 10.0, 0.01);
    close(line[1][0], 30.0, 0.01);
    assert!(!off.tracks_sides());
    let off = off.finish(&plan, &part);
    // A part with no faces under its own material: `simulate` keeps no side lines, and keeping
    // them changes nothing.
    assert_eq!(simulate(&text, &machine, &[flat(6.0)], &stock(), &part, &params(0.01, 0.1)).unwrap(), off);
    // Keeping them changes nothing but the side lines the report hands on to a next setup.
    let mut on = on.finish(&plan, &part);
    assert_eq!(on.sides.len(), 2);
    assert!(off.sides.is_empty());
    on.sides.clear();
    assert_eq!(on, off);
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

/// The stock an earlier setup left, read a hair inside the part (a map on cells of its own, a
/// carried view that takes a millimetre of the part for air), starts held up to the part: a
/// program that never touches the part cuts nothing into it, and one that does is still
/// measured against the part.
#[test]
fn a_carried_stock_inside_the_part_starts_held_up_to_it() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -3.0));
    let machine = Machine::three_axis();
    let start = crate::adaptive::HeightMap { x0: 0.0, y0: 0.0, dx: 0.5, dy: 0.5, nx: 80, ny: 80, heights: vec![-3.03; 6400] };
    let carried = |p: Vec3| p.z < -4.0;
    let air = program("G0 X20. Y20.\nG0 Z5.");
    for c in [None, Some(&carried as &Carried)] {
        let (r, cuts) = simulate_from(&air, &machine, &[flat(6.0)], &stock(), Some(&start), None, c, None, &part, &params(0.01, 0.1), true).unwrap();
        assert_eq!((r.gouge_cells, r.gouge_worst), (0, 0.0), "carried: {}", c.is_some());
        assert!(cuts.is_empty(), "{cuts:?}");
    }
    // A plunge 1 mm into the part's top is a gouge of 1 mm.
    let plunge = program("G0 X20. Y20.\nG0 Z1.\nG1 Z-4. F200\nG0 Z5.");
    let (r, _) =
        simulate_from(&plunge, &machine, &[flat(6.0)], &stock(), Some(&start), None, Some(&carried), None, &part, &params(0.01, 0.1), true).unwrap();
    assert!(r.gouge_cells > 0 && (r.gouge_worst - 1.0).abs() < 0.05, "{} cells, {} mm", r.gouge_cells, r.gouge_worst);
}

/// The stock an earlier setup left, cut down beside a wall of the part on cells of its own (the
/// cell straddling the wall cut, its centre off the part), starts held up to the part along X as
/// well: the side lines hold the part's material out to its wall, and a program that never
/// touches the part cuts nothing into it. (Each line ended at the last cell whose centre is in
/// the part, short of the wall, and the whole wall read as a gouge of the program.)
#[test]
fn a_carried_stock_cut_beside_a_wall_starts_held_up_to_it_along_its_lines() {
    let wall = 20.23;
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(wall, 40.0, -3.0));
    let machine = Machine::three_axis();
    let heights = (0..6400).map(|c| if (c % 80) as f64 * 0.5 + 0.25 < wall { -3.0 } else { -10.0 }).collect();
    let start = crate::adaptive::HeightMap { x0: 0.0, y0: 0.0, dx: 0.5, dy: 0.5, nx: 80, ny: 80, heights };
    let carried = |p: Vec3| p.x < 20.5 || p.z < -10.0;
    let air = program(
        "G0 X10. Y20.
G0 Z5.",
    );
    let (r, cuts) =
        simulate_from(&air, &machine, &[flat(6.0)], &stock(), Some(&start), None, Some(&carried), None, &part, &params(0.01, 0.1), true).unwrap();
    assert_eq!((r.gouge_cells, r.gouge_worst), (0, 0.0));
    assert!(cuts.is_empty(), "{cuts:?}");
    // A cut 0.1 mm into the wall is still a gouge of about that.
    let into = program(
        "G0 X30. Y20.
G0 Z-5.
G1 X23.13 F200
G1 Y10.
G0 Z5.",
    );
    let (r, _) =
        simulate_from(&into, &machine, &[flat(6.0)], &stock(), Some(&start), None, Some(&carried), None, &part, &params(0.01, 0.1), true).unwrap();
    assert!(r.gouge_cells > 0 && (r.gouge_worst - 0.1).abs() < 0.05, "{} cells, {} mm", r.gouge_cells, r.gouge_worst);
}

/// A round boss of radius `r` about `(cx, cy)` from `z0` to `z1`, `n` facets round, closed at
/// both ends, wound outwards.
fn boss_z(cx: f64, cy: f64, r: f64, z0: f64, z1: f64, n: usize) -> Surface {
    let at = |k: usize, z: f64| {
        let a = std::f64::consts::TAU * k as f64 / n as f64;
        Vec3::new(cx + r * a.cos(), cy + r * a.sin(), z)
    };
    let mut tris = Vec::new();
    for k in 0..n {
        let a = std::f64::consts::TAU * (k as f64 + 0.5) / n as f64;
        let out = Vec3::new(a.cos(), a.sin(), 0.0);
        let (p, q, s, t) = (at(k, z0), at(k + 1, z0), at(k + 1, z1), at(k, z1));
        tris.push(facing([p, q, s], out));
        tris.push(facing([p, s, t], out));
        tris.push(facing([Vec3::new(cx, cy, z1), at(k, z1), at(k + 1, z1)], Vec3::new(0.0, 0.0, 1.0)));
        tris.push(facing([Vec3::new(cx, cy, z0), at(k, z0), at(k + 1, z0)], Vec3::new(0.0, 0.0, -1.0)));
    }
    Surface::new(tris).unwrap()
}

/// A carried stock cut beside a curved wall (a round boss; the R5 corner of a turned-over
/// block): each side line along X and along Y meets the wall at its own slant, and the cell
/// straddling the wall on the earlier setup's cells is cut, so a line built from that stock ends
/// inside the part by a share of a cell. Held up to the part, a program that never touches the
/// part cuts nothing into it, and one that cuts 0.1 mm into the wall is still a gouge of about
/// that.
#[test]
fn a_carried_stock_cut_round_a_curved_wall_starts_held_up_to_it_along_its_lines() {
    let (cx, cy, r) = (20.0, 20.0, 12.37);
    let part = boss_z(cx, cy, r, -10.0, -3.0, 720);
    let machine = Machine::three_axis();
    let inside = |x: f64, y: f64| (x - cx).hypot(y - cy) < r;
    let heights = (0..6400).map(|c| if inside((c % 80) as f64 * 0.5 + 0.25, (c / 80) as f64 * 0.5 + 0.25) { -3.0 } else { -10.0 }).collect();
    let start = crate::adaptive::HeightMap { x0: 0.0, y0: 0.0, dx: 0.5, dy: 0.5, nx: 80, ny: 80, heights };
    let carried = move |p: Vec3| (p.x - cx).hypot(p.y - cy) < r + 0.27 || p.z < -10.0;
    let air = program(
        "G0 X20. Y20.
G0 Z5.",
    );
    let (r0, cuts) =
        simulate_from(&air, &machine, &[flat(6.0)], &stock(), Some(&start), None, Some(&carried), None, &part, &params(0.01, 0.1), true).unwrap();
    assert_eq!((r0.gouge_cells, r0.gouge_worst), (0, 0.0), "at {:?}", r0.gouge_at);
    assert!(cuts.is_empty(), "{cuts:?}");
    let into = program(&format!(
        "G0 X40. Y20.
G0 Z-5.
G1 X{:.3} F200
G0 Z5.",
        cx + r + 3.0 - 0.1
    ));
    let (r1, _) =
        simulate_from(&into, &machine, &[flat(6.0)], &stock(), Some(&start), None, Some(&carried), None, &part, &params(0.01, 0.1), true).unwrap();
    assert!(r1.gouge_cells > 0 && (r1.gouge_worst - 0.1).abs() < 0.05, "{} cells, {} mm", r1.gouge_cells, r1.gouge_worst);
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
fn a_rotary_finish_on_a_cylinder_leaves_less_than_the_tolerance() {
    let part = cylinder_x(10.0, -15.0, 15.0, 256);
    let tool = ball(6.0);
    let p = RotaryParams {
        pattern: RotaryPattern::Lines { angle_step: 1f64.to_radians(), zigzag: true },
        clearance: 30.0,
        retract: 14.0,
        floor: 0.0,
        feeds: Feeds { cut: 600.0, plunge: 150.0 },
        range: Some([-8.0, 8.0]),
        step: 1.0,
        tolerance: 0.002,
        stock_to_leave: 0.0,
        max_angle_step: DEFAULT_MAX_ANGLE_STEP,
    };
    let path = rotary_finish(&part, &tool, &p).unwrap();
    let program = Program5 {
        name: "barrel".into(),
        operations: vec![Operation5 { name: "finish".into(), tool: tool.clone(), spindle_rpm: 12000.0, coolant: false, path, plunge_feed: None }],
        work_offset: None,
    };
    let machine = Machine::four_axis_a();
    let text = emit5(&program, &Post5::linuxcnc_4axis(), &machine).unwrap();
    let stock = StockShape::Cylinder { x_min: -8.0, x_max: 8.0, radius: 10.3 };
    let started = Instant::now();
    let r = simulate(&text, &machine, std::slice::from_ref(&tool), &stock, &part, &SimParams { tolerance: 0.01, resolution: None }).unwrap();
    eprintln!(
        "rotary finish: {} cells at {} mm, {:.2} s, leftover max {} mean {}",
        r.cells,
        r.cell,
        started.elapsed().as_secs_f64(),
        r.leftover_max,
        r.leftover_mean
    );
    assert_eq!(r.gouge_cells, 0, "{}", r.gouge_worst);
    assert!(r.rapids_in_material.is_empty(), "{:?}", r.rapids_in_material);
    assert!(r.machined_cells > r.cells / 2, "{} of {}", r.machined_cells, r.cells);
    assert!(r.leftover_max < 0.01, "{}", r.leftover_max);
    close(r.removed_volume, (10.3f64.powi(2) - 10.0f64.powi(2)) * std::f64::consts::PI * 16.0, 6.0);
    // The same program against a fatter part is a gouge, about as deep as the difference.
    let fat = cylinder_x(10.2, -15.0, 15.0, 256);
    let g = simulate(&text, &machine, std::slice::from_ref(&tool), &stock, &fat, &SimParams { tolerance: 0.01, resolution: None }).unwrap();
    assert!(g.gouge_cells > r.cells / 2, "{}", g.gouge_cells);
    close(g.gouge_worst, 0.2, 0.02);
    // The derived stock is the part's cylinder.
    match stock_around(&part, &machine).unwrap() {
        StockShape::Cylinder { x_min, x_max, radius } => {
            close(x_min, -15.0, 1e-9);
            close(x_max, 15.0, 1e-9);
            close(radius, 10.0, 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_rapid_into_round_stock_is_caught() {
    let part = cylinder_x(10.0, -15.0, 15.0, 128);
    let machine = Machine::four_axis_a();
    let stock = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 12.0 };
    let text = "G21\nT1 M6\nG0 X0. Y0. Z30. A0.\nG0 Z11.\nG0 A90.\nG0 Z30.\n";
    let r = simulate(text, &machine, &[ball(6.0)], &stock, &part, &params(0.01, 0.1)).unwrap();
    let lines: Vec<usize> = r.rapids_in_material.iter().map(|m| m.line).collect();
    assert_eq!(lines, vec![4, 5], "{:?}", r.rapids_in_material);
    // Down to 11 at A0 cuts a dimple 1 mm deep at the top: no gouge, the part is at 10.
    assert_eq!(r.gouge_cells, 0);
    // Turning at 9.5 is a gouge, half a millimetre deep.
    let text = "G21\nT1 M6\nG0 X0. Y0. Z30. A0.\nG1 Z9.5 F100\nG1 A90. F2000\nG0 Z30.\n";
    let r = simulate(text, &machine, &[ball(6.0)], &stock, &part, &params(0.01, 0.1)).unwrap();
    assert!(r.rapids_in_material.is_empty());
    close(r.gouge_worst, 0.5, 0.02);
}

/// A square bar on a 4th axis is modelled as the square it is: a rapid across just above a
/// flat side (inside the circle through its corners) cuts nothing, one into it or one turning
/// a corner up into the tool does.
#[test]
fn a_square_bar_on_a_fourth_axis_is_square() {
    let part = block(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0));
    let machine = Machine::four_axis_a();
    let stock = StockShape::Box { min: Vec3::new(-20.0, -15.0, -15.0), max: Vec3::new(20.0, 15.0, 15.0) };
    let text = "G21\nT1 M6\nG0 X-15. Y0. Z30. A0.\nG0 Z18.\nG0 X15.\nG0 Z30.\n";
    let r = simulate(text, &machine, &[flat(6.0)], &stock, &part, &params(0.01, 0.2)).unwrap();
    assert!(r.rapids_in_material.is_empty(), "{:?}", r.rapids_in_material);
    assert_eq!(r.removed_volume, 0.0);
    // Down into the side, or a corner (21.2 mm out) turned up into the tool at Z 18.
    let text = "G21\nT1 M6\nG0 X-5. Y0. Z30. A0.\nG0 Z14.\nG0 Z18.\nG0 A45.\nG0 Z30.\n";
    let r = simulate(text, &machine, &[flat(6.0)], &stock, &part, &params(0.01, 0.2)).unwrap();
    let lines: Vec<usize> = r.rapids_in_material.iter().map(|m| m.line).collect();
    assert_eq!(lines, vec![4, 6], "{:?}", r.rapids_in_material);
    // The cells it did not cut (over the part) are not counted as machined.
    assert!(r.machined_cells > 0 && r.machined_cells < r.cells / 4, "{} of {}", r.machined_cells, r.cells);
    // A box the axis does not run through is refused.
    let off = StockShape::Box { min: Vec3::new(-20.0, -15.0, 1.0), max: Vec3::new(20.0, 15.0, 15.0) };
    assert!(simulate(text, &machine, &[flat(6.0)], &off, &part, &params(0.01, 0.2)).is_err());
}

#[test]
fn derived_stock_and_hostile_input() {
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -5.0));
    match stock_around(&part, &Machine::three_axis()).unwrap() {
        StockShape::Box { min, max } => {
            close3(min, Vec3::new(0.0, 0.0, -10.0), 1e-12);
            close3(max, Vec3::new(40.0, 40.0, -5.0), 1e-12);
        }
        other => panic!("{other:?}"),
    }
    let m = Machine::three_axis();
    let text = program("G1 X10. F100");
    let t = [flat(6.0)];
    // Too many cells, bad tolerances and resolutions, a stock without extent, the wrong stock.
    assert!(matches!(simulate(&text, &m, &t, &stock(), &part, &params(0.01, 0.001)), Err(CamError::TooLarge(_))));
    for p in [params(0.0, 0.1), params(f64::NAN, 0.1), params(0.01, 0.0), params(0.01, f64::INFINITY), params(2.0, 0.1)] {
        assert!(simulate(&text, &m, &t, &stock(), &part, &p).is_err(), "{p:?}");
    }
    let flat_box = StockShape::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(0.0, 10.0, 1.0) };
    assert!(simulate(&text, &m, &t, &flat_box, &part, &params(0.01, 0.1)).is_err());
    let nan_box = StockShape::Box { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::new(1.0, 10.0, 1.0) };
    assert!(simulate(&text, &m, &t, &nan_box, &part, &params(0.01, 0.1)).is_err());
    let cyl = StockShape::Cylinder { x_min: 0.0, x_max: 1.0, radius: 1.0 };
    assert!(simulate(&text, &m, &t, &cyl, &part, &params(0.01, 0.1)).is_err());
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
    let rotary = Machine::four_axis_a();
    let round = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 12.0 };
    for moves in [
        "G0 X0 Y0 Z20 A0\nG1 A99999. F100",
        "G0 X0 Y0 Z20 A0\nG1 X1. Y0. Z-60. A5. F100",
        "G0 X0 Y0 Z20 A0\nG2 X0 Y0.001 I-5 J0 F100",
        "G0 X0 Y0 Z20 A0\nG18 G2 X0 Z20 I999999 K0 F100",
        "G0 X0 Y0 Z20 A0\nG19 G3 Y0 Z20 J-5 K0 F100",
    ] {
        let _ = simulate(&format!("T1 M6\n{moves}\n"), &rotary, &t, &round, &cylinder_x(10.0, -15.0, 15.0, 64), &params(0.01, 0.2));
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
    // The preview mesh is bounded.
    let mesh = r.stock.mesh(256);
    assert!(mesh.positions.len() <= 257 * 257 && mesh.triangles.len() > 1000);
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

#[test]
fn a_program_that_never_writes_the_rotary_is_not_taken_as_checked() {
    // On a 4th axis a program without an A word has no block whose position is known: the
    // report says nothing was simulated rather than looking like a clean pass.
    let part = cylinder_x(10.0, -15.0, 15.0, 64);
    let stock = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 12.0 };
    let text = "G21\nT1 M6\nG0 X0. Y0. Z30.\nG0 Z5.\nG1 X5. F100\nG0 Z30.\n";
    let r = simulate(text, &Machine::four_axis_a(), &[ball(6.0)], &stock, &part, &params(0.01, 0.2)).unwrap();
    assert_eq!(r.moves, 0);
    assert!(r.warnings.iter().any(|w| w.contains("no move")), "{:?}", r.warnings);
    // With the A written the same program is simulated, and its plunge to 5 is a gouge.
    let text = "G21\nT1 M6\nG0 X0. Y0. Z30. A0.\nG0 Z5.\nG1 X5. F100\nG0 Z30.\n";
    let r = simulate(text, &Machine::four_axis_a(), &[ball(6.0)], &stock, &part, &params(0.01, 0.2)).unwrap();
    assert_eq!(r.moves, 4);
    assert!(r.gouge_worst > 4.0, "{}", r.gouge_worst);
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
            let (StockGrid::Heights { heights: a, .. }, StockGrid::Heights { heights: b, .. }) = (&parts.stock, &whole.stock) else { panic!() };
            assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9), "seed {seed}");
            close(parts.removed_volume, whole.removed_volume, 1e-6 * whole.removed_volume.max(1.0));
        }
    }
    // On a 4th axis too.
    let machine = Machine::four_axis_a();
    let round = cylinder_x(10.0, -15.0, 15.0, 64);
    let bar = StockShape::Cylinder { x_min: -10.0, x_max: 10.0, radius: 12.0 };
    let text = "G21
T1 M6
G0 X0. Y0. Z30. A0.
G1 Z10.5 F100
G1 X4. A200. F2000
G1 X-4. A0.
G0 Z30.
";
    let p = params(0.01, 0.2);
    let whole = simulate(text, &machine, &[ball(6.0)], &bar, &round, &p).unwrap();
    for seed in [3, 9] {
        assert_eq!(chunked(text, &machine, &[ball(6.0)], &bar, &round, &p, seed, false), whole);
        // Tool positions along a step are whole pieces of work: exact even part way.
        assert_eq!(chunked(text, &machine, &[ball(6.0)], &bar, &round, &p, seed, true).stock, whole.stock);
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

/// The stock drawn for a view, a tile at a time: the tops of the cells coloured by the tool that
/// cut them last, and a tile the program never reached all uncut.
#[test]
fn the_stock_is_drawn_a_tile_at_a_time_in_the_colour_of_the_tool_that_cut_it() {
    use crate::stock_mesh::{CLASS_TOOL, CLASS_UNCUT, display_step, tile_mesh};
    let plan = plan_sim(&pocket_program(), &Machine::three_axis(), &[flat(6.0)], &stock(), &params(0.01, 0.1)).unwrap();
    let mut state = SimState::new(&plan);
    state.advance(&plan, plan.moves(), 0.0).unwrap();
    let step = display_step(&plan, 1024);
    assert_eq!(step, 1, "400 cells a side fit in 1024");
    assert_eq!(display_step(&plan, 100), 4);
    let tile = tile_mesh(&plan, &state, 12, 12, step);
    assert!(!tile.triangles.is_empty() && tile.tri_face.len() == tile.triangles.len());
    assert!(tile.tri_face.contains(&CLASS_TOOL));
    assert!(tile_mesh(&plan, &state, 0, 0, step).tri_face.iter().all(|c| *c == CLASS_UNCUT));
    // The pocket's floor in the middle tile, at its depth.
    assert!(tile.positions.iter().any(|p| (p.z + 3.0).abs() < 1e-9));
    // Past the grid, nothing.
    assert!(tile_mesh(&plan, &state, 99, 0, step).triangles.is_empty());
}

/// Where the stock is kept along X and Y too (a part with faces under its own material along
/// Z), the view draws what those lines hold that the heights cannot: a slot an earlier setup cut
/// across the part under the stock's top shows its ceiling and its floor; without the lines the
/// column is drawn solid.
#[test]
fn the_stock_view_shows_the_air_the_side_lines_keep_under_material() {
    use crate::stock_mesh::tile_mesh;
    let part = block(Vec3::new(0.0, 0.0, -10.0), Vec3::new(40.0, 40.0, -6.0));
    let text = program(
        "G0 X20. Y20.
G0 Z5.",
    );
    let plan = plan_sim(&text, &Machine::three_axis(), &[flat(6.0)], &stock(), &SimParams { tolerance: 0.01, resolution: Some(0.5) }).unwrap();
    // The slot, y under 20 and z from -6 to -4, cut by the earlier setup.
    let carried = |p: Vec3| !(p.y < 20.0 && p.z > -6.0 && p.z < -4.0);
    // Triangles over the slot facing down at its ceiling and up at its floor, and any facing
    // down at that height past its edge.
    let faces = |state: &SimState| {
        let (tx, ty) = plan.grid().tiles();
        let (mut down, mut up, mut past) = (0usize, 0usize, 0usize);
        for tj in 0..ty {
            for ti in 0..tx {
                let m = tile_mesh(&plan, state, ti, tj, 1);
                for t in &m.triangles {
                    let [a, b, c] = t.map(|i| m.positions[i as usize]);
                    let (u, v) = (b - a, c - a);
                    let n = Vec3::new(0.0, 0.0, u.x * v.y - u.y * v.x);
                    let mid = Vec3::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0, (a.z + b.z + c.z) / 3.0);
                    if n.z < 0.0 && (mid.z + 4.0).abs() < 0.1 {
                        if mid.y < 20.0 { down += 1 } else { past += 1 }
                    }
                    if n.z > 0.0 && (mid.z + 6.0).abs() < 0.1 && mid.y < 20.0 {
                        up += 1;
                    }
                }
            }
        }
        (down, up, past)
    };
    assert_eq!(faces(&SimState::new(&plan)), (0, 0, 0));
    let mut kept = SimState::new(&plan);
    assert!(kept.carve_sides(&plan, &carried, &part));
    // Every display cell over the slot (80 × 40 of them, half a millimetre each), two
    // triangles each, at its ceiling and at its floor; none past it.
    assert_eq!(faces(&kept), (2 * 80 * 40, 2 * 80 * 40, 0));
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

/// The last point of the ray `(x, ρ s, ρ c)` inside the tool, by brute force.
fn brute_exit(p: &Profile, dx: f64, y: f64, z: f64, s: f64, c: f64, cap: f64) -> Option<f64> {
    let n = 400_000;
    (0..=n).rev().map(|k| cap * k as f64 / n as f64).find(|&rho| {
        let d = dx.hypot(rho * s - y);
        d <= p.radius && rho * c >= z + p.height(d)
    })
}

/// A ray leaves the tool (through its side, or its end along the ray) where brute force finds
/// it: the stretch inside the tool is what a cut beside the ray removes.
#[test]
fn rays_leave_the_tool_where_brute_force_finds_it() {
    for tool in [flat(6.0), ball(6.0), bull(6.0, 1.5), drill(6.0)] {
        let p = Profile::of(&tool).unwrap();
        for k in 0..80 {
            let dx = -3.5 + 7.0 * ((k * 13) % 80) as f64 / 80.0;
            let y = -4.0 + 8.0 * ((k * 29) % 80) as f64 / 80.0;
            let z = 5.0 + 10.0 * ((k * 7) % 80) as f64 / 80.0;
            let phi = -1.3 + 2.6 * ((k * 53) % 80) as f64 / 80.0;
            let (s, c) = phi.sin_cos();
            let cap = 30.0;
            let exact = p.ray_span(dx, y, z, s, c, cap);
            let brute = brute_exit(&p, dx, y, z, s, c, cap);
            match (exact, brute) {
                (Some((a, e)), Some(b)) => {
                    assert!(a <= e + 1e-12, "{:?}: enters at {a} after it leaves at {e}", tool.kind);
                    assert!(e >= b - 1e-9, "{:?}: leaves at {e} before {b}", tool.kind);
                    assert!(e - b < 1e-3, "{:?}: leaves at {e} well after {b}", tool.kind);
                }
                (None, None) => {}
                (e, b) => {
                    let touch = e.map(|(_, e)| e).or(b).unwrap_or(0.0);
                    let d = dx.hypot(touch * s - y);
                    assert!((d - 3.0).abs() < 1e-2 || (touch - cap).abs() < 1e-2, "{:?}: {e:?} against {b:?}", tool.kind);
                }
            }
        }
    }
}

/// Passes of a Ø6 flat end mill along X from `x_from` to `x_to` at heights `z` (one per pass)
/// and sideways places `ys`, from Z20 and back up after each: the moves of an indexed operation
/// (the machine's X, Y and Z are the index frame's).
fn passes(ys: &[f64], zs: &[f64], ends: &[(f64, f64)]) -> String {
    let mut t = String::new();
    for ((y, z), (a, b)) in ys.iter().zip(zs).zip(ends) {
        t += &format!("G0 X{a} Y{y}\nG1 Z{z} F300\nG1 X{b} F600\nG0 Z20\n");
    }
    t
}

/// 3+1: an indexed flat on a round bar is modelled in the part's frame. The flat cut right
/// leaves nothing; one pass 0.2 mm too deep is found with its depth and place, by the move
/// watched as it cuts and in what is left; a pass cut short leaves the flat's corner, found
/// with its height and place.
#[test]
fn an_indexed_flat_on_a_round_bar_is_checked() {
    let machine = Machine::four_axis_a();
    let part = flatted_bar(12.0, 9.0, -15.0, 15.0);
    let tool = flat(6.0);
    let params = params(0.01, 0.1);
    let ys = [-6.0, -2.0, 2.0, 6.0];
    // Down into the bar in the middle (beside the rays at a slant through it), then the passes.
    let head = "G21\nT1 M6\nG0 X0 Y0 Z20 A90\nG0 A0\nG1 Z9 F100\nG0 Z20\n";
    let right = format!("{head}{}", passes(&ys, &[9.0; 4], &[(-20.0, 20.0); 4]));
    let stock = StockShape::Cylinder { x_min: -15.0, x_max: 15.0, radius: 12.0 };
    let r = simulate(&right, &machine, std::slice::from_ref(&tool), &stock, &part, &params).unwrap();
    assert_eq!(r.gouge_cells, 0, "{} at {:?}", r.gouge_worst, r.gouge_at);
    assert!(r.leftover_max < 0.02, "{} at {:?}", r.leftover_max, r.leftover_at);
    assert!(r.warnings.iter().all(|w| !w.contains("beside a ray")), "{:?}", r.warnings);
    // The flat's volume: the segment's area (r² (2α − sin 2α) / 2, α its half angle) times 30.
    let alpha = (9.0f64 / 12.0).acos();
    close(r.removed_volume, 144.0 * (2.0 * alpha - (2.0 * alpha).sin()) / 2.0 * 30.0, 30.0);
    // A pass 0.2 mm deep at Y2: found where it is, by the move that cut it.
    let deep = format!("{head}{}", passes(&ys, &[9.0, 9.0, 8.8, 9.0], &[(-20.0, 20.0); 4]));
    let (g, cuts) = crate::sim::simulate_watched(&deep, &machine, std::slice::from_ref(&tool), &stock, &part, &params).unwrap();
    close(g.gouge_worst, 0.2, 0.02);
    let at = g.gouge_at.unwrap();
    assert!((-1.1..=5.1).contains(&at.y) && (at.z - 8.8).abs() < 0.05, "{at:?}");
    assert!(!cuts.is_empty(), "no move was seen cutting into the part");
    let deep_line = deep.lines().enumerate().filter(|(_, l)| l.starts_with("G1 Z8.8")).map(|(n, _)| n + 2).next().unwrap();
    assert!(cuts.iter().all(|(l, _)| *l == deep_line), "{cuts:?} (the deep pass is line {deep_line})");
    close(cuts[0].1, 0.2, 0.02);
    // The last pass cut short at X4: the flat's corner from X7 on, Y5 to its edge, is left.
    let turned = StockShape::Cylinder { x_min: -15.0, x_max: 15.0, radius: 12.5 };
    // Turned to 12 first (a Ø6 flat end mill at Z12, a turn at every 4 mm along X).
    let mut turn = String::from("G21\nT1 M6\nG0 X-12 Y0 Z20 A0\nG1 Z12 F300\n");
    for (k, x) in [-12.0, -8.0, -4.0, 0.0, 4.0, 8.0, 12.0].iter().enumerate() {
        turn += &format!("G1 X{x} F600\nG1 A{} F3000\n", if k % 2 == 0 { 360 } else { 0 });
    }
    turn += "G0 Z20\nG0 A0\n";
    let both = format!("{turn}{}", passes(&ys, &[9.0; 4], &[(-20.0, 20.0), (-20.0, 20.0), (-20.0, 20.0), (-20.0, 4.0)]));
    let left = simulate(&both, &machine, std::slice::from_ref(&tool), &turned, &part, &params).unwrap();
    assert_eq!(left.gouge_cells, 0, "{}", left.gouge_worst);
    let at = left.leftover_at.unwrap();
    assert!(left.leftover_max > 1.0, "{}", left.leftover_max);
    assert!(at.x > 6.9 && at.y > 4.9 && at.z > 9.0, "{at:?}");
    // Not short, the same turned bar is left as the part.
    let whole = format!("{turn}{}", passes(&ys, &[9.0; 4], &[(-20.0, 20.0); 4]));
    let fine = simulate(&whole, &machine, std::slice::from_ref(&tool), &turned, &part, &params).unwrap();
    assert!(fine.leftover_max < 0.02, "{} at {:?}", fine.leftover_max, fine.leftover_at);
}

/// 3+1: a slot cut in the side of a square bar (A at 90°, the machine's X, Y and Z those of the
/// index frame) is left as the same slot cut on a 3-axis machine leaves it: the floor's 0.1 mm
/// of stock and nothing on the walls, with the same volume removed, and no gouge.
#[test]
fn an_indexed_side_slot_simulates_like_the_same_slot_on_three_axes() {
    let tool = flat(6.0);
    let params = params(0.01, 0.1);
    let slot = passes(&[-2.0, 0.0, 2.0], &[15.1; 3], &[(-25.0, 25.0), (25.0, -25.0), (-25.0, 25.0)]);
    let part3 = slotted_bar(-20.0, 20.0);
    let box3 = StockShape::Box { min: Vec3::new(-20.0, -20.0, -20.0), max: Vec3::new(20.0, 20.0, 20.0) };
    let three =
        simulate(&format!("G21\nT1 M6\nG0 X-25 Y-2 Z30\n{slot}"), &Machine::three_axis(), std::slice::from_ref(&tool), &box3, &part3, &params)
            .unwrap();
    let machine = Machine::four_axis_a();
    let frame = crate::indexed::frame_for_index(&machine, &[90f64.to_radians()]).unwrap();
    let part4 = Surface::new(part3.triangles().iter().map(|t| t.map(|p| frame.to_world(p))).collect()).unwrap();
    let started = Instant::now();
    let four = simulate(&format!("G21\nT1 M6\nG0 X-25 Y-2 Z30 A90\n{slot}"), &machine, std::slice::from_ref(&tool), &box3, &part4, &params).unwrap();
    eprintln!(
        "slot: 3-axis leftover {:.4} at {:?}, volume {:.1}; 4th axis leftover {:.4} at {:?}, volume {:.1}, {} cells, {:.2} s",
        three.leftover_max,
        three.leftover_at,
        three.removed_volume,
        four.leftover_max,
        four.leftover_at,
        four.removed_volume,
        four.cells,
        started.elapsed().as_secs_f64()
    );
    assert_eq!(three.gouge_cells, 0);
    assert_eq!(four.gouge_cells, 0, "{} at {:?}", four.gouge_worst, four.gouge_at);
    close(three.leftover_max, 0.1, 0.01);
    assert!((four.leftover_max - three.leftover_max).abs() <= 0.05, "{} against {}", four.leftover_max, three.leftover_max);
    close(four.removed_volume, three.removed_volume, 0.02 * three.removed_volume);
    assert!(four.rapids_in_material.is_empty(), "{:?}", four.rapids_in_material);
    // The slot is in the stock as the view and the checks see it: air in it, material beside it.
    let fp = |p: Vec3| frame.to_world(p);
    let StockGrid::Radii { gaps, .. } = &four.stock else { panic!("not radial") };
    assert!(!gaps.is_empty());
    let plan = plan_sim(&format!("G21\nT1 M6\nG0 X-25 Y-2 Z30 A90\n{slot}"), &machine, std::slice::from_ref(&tool), &box3, &params).unwrap();
    let mut state = SimState::new(&plan);
    state.advance(&plan, plan.moves(), 0.0).unwrap();
    assert!(!state.contains(fp(Vec3::new(0.0, 4.0, 16.0))), "air in the slot, beside its wall");
    assert!(state.contains(fp(Vec3::new(0.0, 6.0, 16.0))), "material beyond its wall");
    assert!(state.contains(fp(Vec3::new(0.0, 4.0, 14.9))), "material under its floor");
    // A pass 0.3 mm deeper, 0.2 mm into the floor, is a gouge, found where it is.
    let deep = passes(&[-2.0, 0.0, 2.0], &[15.1, 14.8, 15.1], &[(-25.0, 25.0), (25.0, -25.0), (-25.0, 25.0)]);
    let g = simulate(&format!("G21\nT1 M6\nG0 X-25 Y-2 Z30 A90\n{deep}"), &machine, std::slice::from_ref(&tool), &box3, &part4, &params).unwrap();
    close(g.gouge_worst, 0.2, 0.02);
    let at = g.gouge_at.unwrap();
    let local = Vec3::new((at - frame.origin).dot(frame.x), (at - frame.origin).dot(frame.y), (at - frame.origin).dot(frame.z));
    assert!(local.y.abs() <= 3.1 && (local.z - 14.8).abs() < 0.05, "{local:?}");
}

/// 3+1: a rapid back along a slot just cut in the side of a square bar, at the slot's depth, runs
/// in the air the slot left (only the scallops between the cut's tool positions are left to it,
/// less than the tolerance thick), as on a 3-axis machine: it is no rapid in material. A rapid
/// sideways into the slot's wall is.
#[test]
fn a_rapid_back_along_an_indexed_slot_is_in_air() {
    let tool = flat(6.0);
    let params = params(0.01, 0.1);
    let mut slot = String::new();
    for y in [-2.0, 0.0, 2.0] {
        slot += &format!("G0 X-25 Y{y}\nG1 Z15.1 F300\nG1 X25 F600\nG0 X-25\nG0 Z30\n");
    }
    let part3 = slotted_bar(-20.0, 20.0);
    let box3 = StockShape::Box { min: Vec3::new(-20.0, -20.0, -20.0), max: Vec3::new(20.0, 20.0, 20.0) };
    let three =
        simulate(&format!("G21\nT1 M6\nG0 X-25 Y-2 Z30\n{slot}"), &Machine::three_axis(), std::slice::from_ref(&tool), &box3, &part3, &params)
            .unwrap();
    assert!(three.rapids_in_material.is_empty(), "3-axis: {:?}", three.rapids_in_material);
    let machine = Machine::four_axis_a();
    let frame = crate::indexed::frame_for_index(&machine, &[90f64.to_radians()]).unwrap();
    let part4 = Surface::new(part3.triangles().iter().map(|t| t.map(|p| frame.to_world(p))).collect()).unwrap();
    let four = simulate(&format!("G21\nT1 M6\nG0 X-25 Y-2 Z30 A90\n{slot}"), &machine, std::slice::from_ref(&tool), &box3, &part4, &params).unwrap();
    assert!(four.rapids_in_material.is_empty(), "4th axis: {:?}", four.rapids_in_material);
    let wall = format!("G21\nT1 M6\nG0 X-25 Y-2 Z30 A90\n{slot}G0 X0 Y0\nG0 Z16\nG0 Y4\n");
    let into = simulate(&wall, &machine, std::slice::from_ref(&tool), &box3, &part4, &params).unwrap();
    assert!(!into.rapids_in_material.is_empty(), "a rapid into the slot's wall was not found");
}

/// On a 4th axis the stock left along a ray is measured down to the part along that ray, and
/// no further than the axis: a ray that meets no part before the axis (down a hole across the
/// bar not drilled yet, or past the axis to a part on the far side) has at most its own material
/// left, never more than the bar's radius (it read 24 mm on a 16 mm bar, from the far side of
/// a cross hole).
#[test]
fn leftover_on_a_ray_is_no_more_than_its_material() {
    let machine = Machine::four_axis_a();
    let tool = flat(6.0);
    // The part lies below the axis only: the rays that point up meet it past the axis.
    let part = block(Vec3::new(-15.0, -12.0, -12.0), Vec3::new(15.0, 12.0, -6.0));
    let stock = StockShape::Cylinder { x_min: -15.0, x_max: 15.0, radius: 12.0 };
    let mut turn = String::from("G21\nT1 M6\nG0 X-12 Y0 Z20 A0\nG1 Z11.5 F300\n");
    for (k, x) in [-12.0, -8.0, -4.0, 0.0, 4.0, 8.0, 12.0].iter().enumerate() {
        turn += &format!("G1 X{x} F600\nG1 A{} F3000\n", if k % 2 == 0 { 360 } else { 0 });
    }
    turn += "G0 Z20\n";
    let r = simulate(&turn, &machine, std::slice::from_ref(&tool), &stock, &part, &params(0.01, 0.1)).unwrap();
    assert!(r.leftover_max <= 11.5 + 0.05, "{} mm left at {:?} on a bar turned to 11.5", r.leftover_max, r.leftover_at);
    assert!(r.leftover_max > 11.0, "{}: the rays that meet no part keep all their material", r.leftover_max);
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

/// How far the part reaches from the A axis per column (what lets the comparison pass over the
/// rays the program never cut that end beyond the part): no less than the part there, nothing
/// where it is not.
#[test]
fn the_part_reach_per_column_bounds_the_part() {
    let part = cylinder_x(10.0, -15.0, 15.0, 64);
    // Columns 1 mm wide from X -20: centres -19.5 to 19.5.
    let reach = crate::sim::column_reach(&part, -20.0, 1.0, 40);
    for (i, r) in reach.iter().enumerate() {
        let x = -19.5 + i as f64;
        if x.abs() < 15.0 {
            assert!(*r >= 10.0 - 1e-9 && *r < 10.1, "{x}: {r}");
        } else {
            assert_eq!(*r, 0.0, "{x}");
        }
    }
    // Hostile sizes give nothing, never a panic.
    assert!(crate::sim::column_reach(&part, -20.0, 0.0, 40).iter().all(|r| *r == 0.0));
    assert!(crate::sim::column_reach(&part, -20.0, f64::NAN, 40).iter().all(|r| *r == 0.0));
    assert!(crate::sim::column_reach(&part, -20.0, 1.0, 0).is_empty());
}

#[test]
fn tools_are_drawn_from_their_profile() {
    let held = Tool {
        stickout: Some(30.0),
        holder: vec![HolderSection { diameter: 34.0, length: 4.0 }, HolderSection { diameter: 34.0, length: 18.0 }],
        ..Tool::new(1, ToolKind::BallEndmill, 6.0, 12.0)
    };
    let m = crate::tool_mesh::tool_meshes(&held, 24).unwrap();
    let top = |mesh: &solidcraft_geom::Mesh| mesh.positions.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
    let low = |mesh: &solidcraft_geom::Mesh| mesh.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    close(low(&m.cutting), 0.0, 1e-12);
    close(top(&m.cutting), 12.0, 1e-12);
    close(top(&m.shank), 30.0, 1e-12);
    close(top(&m.holder), 52.0, 1e-12);
    // The ball's points are on its sphere.
    assert!(
        m.cutting.positions.iter().filter(|p| p.z < 3.0 - 1e-9).all(|p| ((p.x * p.x + p.y * p.y + (p.z - 3.0).powi(2)).sqrt() - 3.0).abs() < 1e-9)
    );
    let thread = Tool { thread_pitch: Some(1.5), tooth_height: Some(0.8), ..Tool::new(2, ToolKind::ThreadMill, 8.0, 9.0) };
    assert!(
        crate::tool_mesh::tool_meshes(&thread, 16).unwrap().cutting.positions.len()
            > crate::tool_mesh::tool_meshes(&Tool::new(3, ToolKind::FlatEndmill, 8.0, 9.0), 16).unwrap().cutting.positions.len()
    );
}

/// Every kind of tool the view draws, each with its closed-form height above the tip at a
/// distance from its axis (written here from the tools' sizes, not taken from the simulation).
type HeightAt = Box<dyn Fn(f64) -> f64>;
fn kinds() -> Vec<(Tool, HeightAt)> {
    let r = 3.0;
    let bull = 1.0;
    let drill = 1.0 / (118f64.to_radians() * 0.5).tan();
    let face_corner = 0.8;
    let corner = move |c: f64| move |d: f64| if d <= r - c { 0.0 } else { c - (c * c - (d - (r - c)).powi(2)).max(0.0).sqrt() };
    vec![
        (Tool::new(1, ToolKind::FlatEndmill, 6.0, 20.0), Box::new(|_d: f64| 0.0)),
        (Tool::new(2, ToolKind::BallEndmill, 6.0, 20.0), Box::new(move |d: f64| r - (r * r - d * d).max(0.0).sqrt())),
        (Tool { corner_radius: Some(bull), ..Tool::new(3, ToolKind::BullNose, 6.0, 20.0) }, Box::new(corner(bull))),
        (Tool { angle_deg: Some(90.0), ..Tool::new(4, ToolKind::Chamfer, 6.0, 20.0) }, Box::new(|d: f64| d)),
        (Tool { point_angle_deg: Some(118.0), ..Tool::new(5, ToolKind::Drill, 6.0, 20.0) }, Box::new(move |d: f64| drill * d)),
        (Tool { corner_radius: Some(face_corner), ..Tool::new(6, ToolKind::FaceMill, 6.0, 20.0) }, Box::new(corner(face_corner))),
    ]
}

#[test]
fn every_kind_of_tool_is_drawn_with_its_own_profile_and_size() {
    for (t, h) in kinds() {
        let m = crate::tool_mesh::tool_meshes(&t, 48).unwrap();
        let r = t.diameter * 0.5;
        let widest = m.cutting.positions.iter().map(|p| p.x.hypot(p.y)).fold(0.0, f64::max);
        assert!((widest - r).abs() < 1e-9, "{:?}: drawn {} mm across, is {} mm", t.kind, 2.0 * widest, t.diameter);
        let lowest = m.cutting.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        assert!(lowest.abs() < 1e-12, "{:?}: the tip is drawn at {lowest}, not at the origin", t.kind);
        // Every point is on the cutting end's surface, on the side, or on the top's centre.
        for p in &m.cutting.positions {
            let rho = p.x.hypot(p.y);
            let on_end = (p.z - h(rho)).abs() < 1e-9;
            let on_side = (rho - r).abs() < 1e-9 && p.z >= h(r) - 1e-9;
            let top = rho < 1e-9 && (p.z - t.flute_length).abs() < 1e-9;
            assert!(on_end || on_side || top, "{:?}: a point at r {rho}, z {} is off the tool", t.kind, p.z);
        }
        assert!(m.cutting.positions.iter().all(|p| p.z <= t.flute_length + 1e-9), "{:?}", t.kind);
    }
}

/// Findings merged by their code: counted, the first lines kept once each and no more than
/// [`crate::findings::MAX_LINES`], the deepest kept, errors listed first; a log keeps its events
/// in move order, and merges those past its cap.
#[test]
fn findings_merge_by_code_and_a_log_keeps_them_by_move() {
    use crate::findings::{FindingEvent, FindingLog, Findings, MAX_EVENTS, MAX_LINES, Severity};
    let event = |code: &str, severity: Severity, at_move: Option<usize>, line: usize, depth: f64| FindingEvent {
        code: code.into(),
        severity,
        at_move,
        line,
        op: None,
        at: None,
        depth,
        message: format!("{code} at line {line}"),
    };
    let mut f = Findings::default();
    assert!(f.is_empty());
    f.add(&event("air-cut", Severity::Info, Some(1), 12, 0.0));
    f.add(&event("rapid-in-stock", Severity::Error, Some(2), 14, 0.5));
    f.add(&event("rapid-in-stock", Severity::Error, Some(3), 14, 1.5));
    f.add(&event("rapid-in-stock", Severity::Error, Some(4), 20, 0.2));
    assert!(f.has("rapid-in-stock") && !f.has("gouge"));
    let list = f.list();
    assert_eq!(list.iter().map(|x| x.code.as_str()).collect::<Vec<_>>(), ["rapid-in-stock", "air-cut"]);
    assert_eq!((list[0].count, list[0].lines.clone(), list[0].worst), (3, vec![14, 20], 1.5));
    assert_eq!(list[0].message, "rapid-in-stock at line 14");
    assert_eq!((f.count(Severity::Error), f.count(Severity::Warning), f.count(Severity::Info)), (3, 0, 1));
    for line in 100..200 {
        f.add(&event("air-cut", Severity::Info, None, line, 0.0));
    }
    assert_eq!(f.list()[1].lines.len(), MAX_LINES);
    let mut g = Findings::default();
    g.add(&event("gouge", Severity::Error, Some(9), 30, 0.3));
    g.add(&event("rapid-in-stock", Severity::Error, Some(9), 31, 2.0));
    f.merge(&g);
    let list = f.list();
    assert_eq!(list.iter().map(|x| x.code.as_str()).collect::<Vec<_>>(), ["rapid-in-stock", "gouge", "air-cut"]);
    assert_eq!((list[0].count, list[0].worst), (4, 2.0));
    // A log in move order: the setup's events first, then each move's in the order found.
    let mut log = FindingLog::default();
    log.push(event("gouge", Severity::Error, Some(5), 40, 0.1));
    log.push(event("holder-hits-stock", Severity::Error, None, 0, 0.0));
    log.push(event("gouge", Severity::Error, Some(2), 33, 0.4));
    log.sort();
    assert_eq!(log.events().iter().map(|e| e.at_move).collect::<Vec<_>>(), [None, Some(2), Some(5)]);
    assert_eq!(log.until(3).count(), 2);
    assert_eq!(log.merged_until(3).list().iter().map(|x| x.count).sum::<usize>(), 2);
    for k in 0..MAX_EVENTS + 5 {
        log.push(event("air-cut", Severity::Info, Some(k), k + 1, 0.0));
    }
    assert_eq!(log.len(), MAX_EVENTS);
    assert_eq!(log.dropped().count(Severity::Info), 8);
    assert_eq!(log.merged().count(Severity::Info), MAX_EVENTS + 5);
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
