use std::f64::consts::{FRAC_PI_2, PI};

use solidcraft_geom::{Aabb2, Loop2, Region2, Seg2, Vec2, Vec3};

use crate::{Anchor, Bend, Datum, DevelopError, Joint, JointKind, Panel, Rigid2, Skin, Surface, Tail, ZoneEnds, develop};

const T: f64 = 2.0;
const R: f64 = 2.0;
const K: f64 = 0.5;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    let pts = [Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)];
    Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() }
}

fn bounds(r: &Region2) -> Aabb2 {
    r.outer.polyline(1e-6).bounds()
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-9
}

fn same_motion(a: &Rigid2, b: &Rigid2) -> bool {
    near(a.point(Vec2::ZERO), b.point(Vec2::ZERO)) && near(a.point(Vec2::X), b.point(Vec2::X))
}

fn bend(angle: f64, datum: Datum, span: (f64, f64)) -> Bend {
    Bend {
        angle,
        radius: R,
        toward_material: true,
        datum,
        ends: ZoneEnds { near: span, far: span },
        tails: (Tail::Keep, Tail::Trim),
        relieved: (false, false),
    }
}

/// An L bracket: a 100 x 60 base whose virtual sharp runs along y = 0, and a 20 mm flange drawn
/// in a chart of its own, turned and shifted so that nothing about its flat place is given
/// away. The flange's material lies on the right of its anchor.
fn bracket(b: Bend) -> Skin {
    let chart = Rigid2::onto(Vec2::ZERO, Vec2::X, Vec2::new(7.0, -3.0), Vec2::from_angle(1.1)).unwrap_or(Rigid2::IDENTITY);
    Skin {
        thickness: T,
        k_factor: K,
        panels: vec![
            Panel { name: "base".into(), region: rect(0.0, 0.0, 100.0, 60.0), ..Panel::default() },
            Panel { name: "flange".into(), region: chart.region(&rect(0.0, -20.0, 100.0, 0.0)), ..Panel::default() },
        ],
        joints: vec![Joint {
            a: Anchor { panel: 0, origin: Vec2::ZERO, dir: Vec2::X },
            b: Anchor { panel: 1, origin: chart.point(Vec2::ZERO), dir: chart.vector(Vec2::X) },
            kind: JointKind::Bend(b),
        }],
    }
}

#[test]
fn a_rigid_motion_maps_every_kind_of_segment_exactly() {
    let m = Rigid2::onto(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0), Vec2::new(-5.0, 0.5), Vec2::new(-1.0, 2.0)).unwrap_or(Rigid2::IDENTITY);
    let mut p = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
    for (i, q) in p.iter_mut().take(5).enumerate() {
        *q = Vec2::new(i as f64, (i * i) as f64 * 0.3);
    }
    let segs = [
        Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(4.0, 1.0) },
        Seg2::Arc { center: Vec2::new(2.0, -1.0), radius: 3.0, start: 0.4, sweep: -2.2 },
        Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(1.0, 3.0), p2: Vec2::new(4.0, -2.0), p3: Vec2::new(5.0, 1.0) },
        Seg2::Conic { a: Vec2::new(1.0, 0.0), apex: Vec2::new(1.0, 1.0), b: Vec2::new(0.0, 1.0), w: 0.5f64.sqrt() },
        Seg2::Bezier { n: 4, p },
    ];
    for s in &segs {
        for t in [0.0, 0.25, 0.6, 1.0] {
            assert!(near(m.seg(s).point_at(t), m.point(s.point_at(t))), "{s:?} at {t}");
        }
    }
    assert!(same_motion(&m.then(&m.inverse()), &Rigid2::IDENTITY));
    assert!(same_motion(&m.inverse().then(&m), &Rigid2::IDENTITY));
}

/// A 90 degree bend toward the material, anchored at the outer virtual sharp: each panel is
/// set back (R + T) tan 45 = 4 from the sharp, the zone is (pi / 2)(R + K T) wide, and the
/// flat is the outside lengths less the bend deduction 2 x 4 - pi / 2 x 3.
#[test]
fn an_l_bracket_lays_out_with_its_bend_deduction() {
    let skin = bracket(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0)));
    let dev = develop(&skin, 0).unwrap_or_default();
    let ba = FRAC_PI_2 * (R + K * T);
    let Some(Some(flange)) = dev.placements.get(1) else { panic!("the flange is placed: {dev:?}") };
    let Some(region) = skin.panels.get(1).map(|p| flange.region(&p.region)) else { panic!() };
    // The flange's sharp lands 8 - BA below the base's (the flat frame's y runs into the base).
    let b = bounds(&region);
    assert!((b.max.y - (8.0 - ba)).abs() < 1e-9 && (b.min.y - (8.0 - ba - 20.0)).abs() < 1e-9, "{b:?}");
    assert!(b.min.x.abs() < 1e-9 && (b.max.x - 100.0).abs() < 1e-9, "{b:?}");
    // The whole flat: 60 + 20 less the deduction.
    assert!(((60.0 - b.min.y) - (80.0 - (8.0 - ba))).abs() < 1e-9);
    // The zone runs from the base's tangent line, 4 into the base, BA toward the flange.
    let [zone] = dev.zones.as_slice() else { panic!("one zone: {dev:?}") };
    assert_eq!(zone.joint, 0);
    let z = bounds(&zone.region);
    assert!((z.max.y - 4.0).abs() < 1e-9 && (z.min.y - (4.0 - ba)).abs() < 1e-9 && (zone.region.area() - 100.0 * ba).abs() < 1e-9, "{z:?}");
    // The base loses what lies between its tangent line and the sharp along the zone, and keeps
    // its tail; the flange loses its own along the zone and past both ends.
    let base: Vec<&Region2> = dev.trims.iter().filter(|t| t.panel == 0).map(|t| &t.region).collect();
    let [band] = base.as_slice() else { panic!("one trim of the base: {:?}", dev.trims) };
    let bb = bounds(band);
    assert!((bb.max.y - 4.0).abs() < 1e-9 && bb.min.y < 0.0, "{bb:?}");
    let flange: Vec<&Region2> = dev.trims.iter().filter(|t| t.panel == 1).map(|t| &t.region).collect();
    let [band] = flange.as_slice() else { panic!("one trim of the flange: {:?}", dev.trims) };
    let fb = bounds(band);
    assert!(fb.min.x < 0.0 && fb.max.x > 100.0 && (fb.min.y - (4.0 - ba)).abs() < 1e-9, "{fb:?}");
    assert!(dev.loops.is_empty());
}

/// Laid out from the flange instead, the base lands where the flange's chart puts it: the two
/// layouts differ by one rigid motion.
#[test]
fn any_panel_can_be_the_root() {
    let skin = bracket(bend(1.2, Datum::Sharp, (10.0, 90.0)));
    let from_base = develop(&skin, 0).unwrap_or_default();
    let from_flange = develop(&skin, 1).unwrap_or_default();
    let (Some(Some(flange)), Some(Some(base))) = (from_base.placements.get(1), from_flange.placements.first()) else { panic!() };
    assert!(same_motion(base, &flange.inverse()));
}

/// A hem has no virtual sharp: anchored at its tangent lines, the flange's starts BA past the
/// base's. Anchored at a sharp it is refused.
#[test]
fn a_hem_is_anchored_at_its_tangent_lines() {
    let mut hem = bend(PI, Datum::Tangent, (0.0, 100.0));
    hem.radius = 0.01;
    let skin = bracket(hem.clone());
    let dev = develop(&skin, 0).unwrap_or_default();
    let ba = PI * (0.01 + K * T);
    let Some(Some(flange)) = dev.placements.get(1) else { panic!() };
    let Some(region) = skin.panels.get(1).map(|p| flange.region(&p.region)) else { panic!() };
    assert!((bounds(&region).max.y + ba).abs() < 1e-9, "{:?}", bounds(&region));
    hem.datum = Datum::Sharp;
    assert!(matches!(develop(&bracket(hem), 0), Err(DevelopError::Joint(0, _))));
}

/// A torn end: the zone's edge on the flange side ends where the flange's panel does, so the
/// zone is a trapezoid.
#[test]
fn a_torn_end_slants_the_zone() {
    let mut b = bend(FRAC_PI_2, Datum::Sharp, (5.0, 95.0));
    b.ends.far = (0.0, 95.0);
    let dev = develop(&bracket(b), 0).unwrap_or_default();
    let ba = FRAC_PI_2 * (R + K * T);
    let [zone] = dev.zones.as_slice() else { panic!() };
    assert!((zone.region.area() - ba * (90.0 + 95.0) / 2.0).abs() < 1e-9, "{}", zone.region.area());
}

/// A zone that stops short of the base's edge: past it the base keeps its tail, out to its sharp,
/// unless that end is a corner relief, where the base stops at its tangent line.
#[test]
fn a_relieved_end_stops_the_base_at_its_tangent_line() {
    let base_band = |relieved: (bool, bool)| {
        let mut b = bend(FRAC_PI_2, Datum::Sharp, (5.0, 95.0));
        b.relieved = relieved;
        let dev = develop(&bracket(b), 0).unwrap_or_default();
        let bands: Vec<Aabb2> = dev.trims.iter().filter(|t| t.panel == 0).map(|t| bounds(&t.region)).collect();
        let [band] = bands.as_slice() else { panic!("one trim of the base: {:?}", dev.trims) };
        (band.min.x, band.max.x)
    };
    let (kept, relieved) = (base_band((false, false)), base_band((true, false)));
    assert!((kept.0 - 5.0).abs() < 1e-9 && (kept.1 - 95.0).abs() < 1e-9, "{kept:?}");
    assert!(relieved.0 < 0.0 && (relieved.1 - 95.0).abs() < 1e-9, "{relieved:?}");
}

/// Four walls of a square tube joined by four bends close a ring: the layout follows three of
/// them and reports how far the fourth is from fitting, since a ring only flattens with a rip.
#[test]
fn a_ring_of_bends_is_reported_not_forced() {
    let wall = |i: usize| Panel { name: format!("wall {i}"), region: rect(0.0, 0.0, 100.0, 40.0), ..Panel::default() };
    let joint = |a: usize, b: usize| Joint {
        a: Anchor { panel: a, origin: Vec2::new(100.0, 40.0), dir: Vec2::new(-1.0, 0.0) },
        b: Anchor { panel: b, origin: Vec2::new(100.0, 0.0), dir: Vec2::new(-1.0, 0.0) },
        kind: JointKind::Bend(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0))),
    };
    let skin =
        Skin { thickness: T, k_factor: K, panels: (0..4).map(wall).collect(), joints: vec![joint(0, 1), joint(1, 2), joint(2, 3), joint(3, 0)] };
    let dev = develop(&skin, 0).unwrap_or_default();
    assert!(dev.placements.iter().all(Option::is_some));
    let [miss] = dev.loops.as_slice() else { panic!("one loop: {:?}", dev.loops) };
    // Laid out from wall 0 both ways round, walls 1 and 2 go one way and wall 3 the other, so
    // the bend between walls 2 and 3 closes the ring: through it, wall 3 would lie four walls
    // and four bends from where the other way put it.
    let step = 40.0 + FRAC_PI_2 * (R + K * T) - 8.0;
    assert!(miss.joint == 2 && (miss.distance - 4.0 * step).abs() < 1e-9 && miss.angle < 1e-12, "{miss:?}");
    let ring_free = Skin { joints: skin.joints.iter().take(3).cloned().collect(), ..skin.clone() };
    assert!(develop(&ring_free, 0).unwrap_or_default().loops.is_empty());
}

/// A rip is not followed: its panels are laid out as pieces of their own, the second beside the
/// first, and each loses its cut along the rip.
#[test]
fn a_rip_lays_its_panels_out_apart_and_bad_joints_are_refused() {
    let mut skin = bracket(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0)));
    if let Some(j) = skin.joints.get_mut(0) {
        j.kind = JointKind::Rip { cut: (0.5, 0.25) };
    }
    let dev = develop(&skin, 0).unwrap_or_default();
    assert_eq!(dev.pieces, vec![vec![0], vec![1]]);
    assert!(dev.zones.is_empty() && dev.loops.is_empty());
    // The flange lies ten thicknesses to the right of the base, level with its lowest edge.
    let Some(Some(flange)) = dev.panels.get(1) else { panic!("the flange is laid out: {dev:?}") };
    let fb = bounds(&flange.region);
    assert!((fb.min.x - 120.0).abs() < 1e-9 && fb.min.y.abs() < 1e-9, "{fb:?}");
    // The base loses 0.5 along the rip, into its material above y = 0.
    let [base, flange] = dev.trims.as_slice() else { panic!("a trim of each: {:?}", dev.trims) };
    assert!(base.panel == 0 && flange.panel == 1 && (base.line[0].y - 0.5).abs() < 1e-9);
    let bb = bounds(&base.region);
    assert!(bb.min.y.abs() < 1e-9 && (bb.max.y - 0.5).abs() < 1e-9, "{bb:?}");
    assert_eq!(develop(&skin, 5), Err(DevelopError::NoPanel(5)));
    if let Some(j) = skin.joints.get_mut(0) {
        j.b.dir = Vec2::new(2.0, 0.0);
    }
    assert!(matches!(develop(&skin, 0), Err(DevelopError::Joint(0, _))));
}

/// A roll `length` long along its axis, turning through `angle` (radians) round it.
fn roll(radius: f64, material_inward: bool, length: f64, angle: f64) -> Panel {
    Panel { name: "roll".into(), surface: Surface::Cylinder { radius, material_inward }, region: rect(0.0, 0.0, length, angle) }
}

/// Panels 100 long whose seams run along x: `a`'s at `ya` with its material below, `b`'s at
/// `yb` with its material above.
fn seam(a: usize, ya: f64, b: usize, yb: f64) -> Joint {
    Joint {
        a: Anchor { panel: a, origin: Vec2::new(100.0, ya), dir: Vec2::new(-1.0, 0.0) },
        b: Anchor { panel: b, origin: Vec2::new(100.0, yb), dir: Vec2::new(-1.0, 0.0) },
        kind: JointKind::Seam,
    }
}

/// A floor, a quarter roll and a wall joined by seams: the roll flattens to its neutral arc's
/// length, a quarter turn at K x T out from the inside of the roll. With the skin on the inside
/// of the roll the neutral surface lies further out than the skin; with it on the outside,
/// further in.
#[test]
fn a_roll_flattens_through_its_neutral_surface() {
    let r = 30.0;
    for (inward, neutral_radius) in [(false, r + K * T), (true, r - (1.0 - K) * T)] {
        let skin = Skin {
            thickness: T,
            k_factor: K,
            panels: vec![
                Panel { name: "floor".into(), region: rect(0.0, 0.0, 100.0, 40.0), ..Panel::default() },
                roll(r, inward, 100.0, FRAC_PI_2),
                Panel { name: "wall".into(), region: rect(0.0, 0.0, 100.0, 30.0), ..Panel::default() },
            ],
            joints: vec![seam(0, 40.0, 1, 0.0), seam(1, FRAC_PI_2, 2, 0.0)],
        };
        let dev = develop(&skin, 0).unwrap_or_default();
        assert!(dev.loops.is_empty() && dev.zones.is_empty() && dev.trims.is_empty() && dev.pieces.len() == 1);
        let Some(Some(wall)) = dev.panels.get(2) else { panic!("the wall is laid out: {dev:?}") };
        let flat = bounds(&wall.region).max.y;
        assert!((flat - (40.0 + FRAC_PI_2 * neutral_radius + 30.0)).abs() < 1e-9, "{flat}");
    }
}

/// A cone opened along a ruling flattens to a sector of a ring. Drawn in its parameters
/// (distance from the apex, angle round the axis), the panel is a rectangle: its rulings stay
/// lines, and its circles round the axis become arcs about the apex that keep their sweep
/// (a whole turn times the sine of the half angle) and move by the neutral surface's offset along
/// the rulings, K x T / tan(half angle).
#[test]
fn a_cone_flattens_to_a_sector_at_its_neutral_radii() {
    let half_angle: f64 = 0.5;
    let turn = std::f64::consts::TAU;
    let sweep = turn * half_angle.sin();
    let skin = Skin {
        thickness: T,
        k_factor: K,
        panels: vec![Panel {
            name: "hopper".into(),
            surface: Surface::Cone { half_angle, material_inward: false },
            region: rect(40.0, 0.0, 90.0, turn),
        }],
        joints: Vec::new(),
    };
    let dev = develop(&skin, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    let d = K * T / half_angle.tan();
    let arcs: Vec<(f64, f64)> =
        flat.region.outer.segs.iter().filter_map(|s| if let Seg2::Arc { radius, sweep, .. } = s { Some((*radius, *sweep)) } else { None }).collect();
    assert_eq!(arcs.len(), 2, "{:?}", flat.region.outer.segs);
    assert!(arcs.iter().any(|&(r, s)| (r - (90.0 + d)).abs() < 1e-12 && (s - sweep).abs() < 1e-12), "{arcs:?}");
    assert!(arcs.iter().any(|&(r, s)| (r - (40.0 + d)).abs() < 1e-12 && (s + sweep).abs() < 1e-12), "{arcs:?}");
    assert_eq!(flat.region.outer.segs.len(), 4, "the rulings stay single lines");
}

/// Two half rolls joined along both their edges close a tube: the layout reports the ring. With
/// one seam a rip, the tube opens flat, a whole turn of the neutral circle wide.
#[test]
fn a_tube_of_rolls_opens_with_a_rip() {
    let r = 25.0;
    let mut skin = Skin {
        thickness: T,
        k_factor: K,
        panels: vec![roll(r, true, 100.0, PI), roll(r, true, 100.0, PI)],
        joints: vec![seam(0, PI, 1, 0.0), seam(1, PI, 0, 0.0)],
    };
    assert_eq!(develop(&skin, 0).unwrap_or_default().loops.len(), 1);
    if let Some(j) = skin.joints.get_mut(1) {
        j.kind = JointKind::Rip { cut: (0.0, 0.0) };
    }
    let dev = develop(&skin, 0).unwrap_or_default();
    assert!(dev.loops.is_empty() && dev.pieces.len() == 1, "{dev:?}");
    let Some(Some(second)) = dev.panels.get(1) else { panic!() };
    let around = std::f64::consts::TAU * (r - (1.0 - K) * T);
    assert!((bounds(&second.region).max.y - around).abs() < 1e-9, "{:?}", bounds(&second.region));
}

/// A round hole on a roll, 10 mm across the skin, is an ellipse in the roll's parameters (the
/// angle round it is length over the radius). Flattened, it lies on the neutral circle: still an
/// exact ellipse, as conics that each say which of the hole's segments they came from.
#[test]
fn a_hole_in_a_roll_flattens_to_an_exact_ellipse() {
    let (r, c, hole) = (40.0, Vec2::new(50.0, 30.0), 10.0);
    let on_skin = |p: Vec2| Vec2::new(p.x, p.y / r);
    let quarters: Vec<Seg2> = crate::chart::arc_as_conics(c, hole, 0.0, std::f64::consts::TAU)
        .into_iter()
        .map(|s| match s {
            Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: on_skin(a), apex: on_skin(apex), b: on_skin(b), w },
            other => other,
        })
        .collect();
    let mut panel = roll(r, false, 100.0, 60.0 / r);
    panel.region.holes.push(Loop2 { segs: quarters }.reversed());
    let skin = Skin { thickness: T, k_factor: K, panels: vec![panel], joints: Vec::new() };
    let dev = develop(&skin, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    let (Some(ellipse), Some(from)) = (flat.region.holes.first(), flat.origins.get(1)) else { panic!("{flat:?}") };
    let k = (r + K * T) / r;
    assert!(ellipse.segs.iter().all(|s| matches!(s, Seg2::Conic { .. })), "{ellipse:?}");
    assert_eq!(from, &[0, 1, 2, 3], "each quarter stays one conic");
    for s in &ellipse.segs {
        for t in [0.0, 0.3, 0.7, 1.0] {
            let p = s.point_at(t);
            let e = ((p.x - c.x) / hole).powi(2) + ((p.y - c.y * k) / (hole * k)).powi(2);
            assert!((e - 1.0).abs() < 1e-12, "{e}");
        }
    }
}

#[test]
fn a_bend_on_the_edge_of_a_roll_is_refused() {
    let mut skin = bracket(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0)));
    if let Some(p) = skin.panels.get_mut(1) {
        p.surface = Surface::Cylinder { radius: 50.0, material_inward: false };
    }
    assert!(matches!(develop(&skin, 0), Err(DevelopError::Joint(0, _))));
}

fn spline(degree: [usize; 2], knots: [Vec<f64>; 2], points: &[Vec3], weights: &[f64]) -> Surface {
    match solidcraft_geom::bspline::NurbsSurface::new(degree, knots, points, weights) {
        Ok(s) => Surface::Spline(std::sync::Arc::new(s)),
        Err(e) => panic!("{e:?}"),
    }
}

/// A free-form panel over its whole parameter square.
fn free(surface: Surface) -> Skin {
    Skin { thickness: T, k_factor: K, panels: vec![Panel { name: "free".into(), surface, region: rect(0.0, 0.0, 1.0, 1.0) }], joints: Vec::new() }
}

/// The flat length of the parameter line from `a` to `b`, along the flattened segments.
fn flat_length(skin: &Skin, a: Vec2, b: Vec2) -> f64 {
    let mut probe = skin.clone();
    if let Some(p) = probe.panels.first_mut() {
        p.region = Region2 { outer: Loop2 { segs: vec![Seg2::Line { a, b }, Seg2::Line { a: b, b: a }] }, holes: Vec::new() };
    }
    let dev = develop(&probe, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    // Every piece the line came back as: one per knot span it crosses.
    let from = flat.origins.first().cloned().unwrap_or_default();
    flat.region.outer.segs.iter().zip(from).filter(|(_, o)| *o == 0).map(|(s, _)| s.length()).sum()
}

/// A quarter of a cylinder of radius 40, 100 long, written as a rational spline (its angle is no
/// parameter of it), its skin on the inside: fitted in its own spline space, it flattens to the
/// neutral arc's length, a quarter turn at 40 + K x T, with next to no stretch.
#[test]
fn a_spline_cylinder_flattens_like_a_roll() {
    let r = 40.0;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for (x, z, wt) in [(r, 0.0, 1.0), (r, r, w), (0.0, r, 1.0)] {
        for y in [0.0, 100.0] {
            points.push(Vec3::new(x, y, z));
            weights.push(wt);
        }
    }
    let skin = free(spline([1, 2], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]], &points, &weights));
    let dev = develop(&skin, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    assert!(flat.stretch < 1e-6, "{}", flat.stretch);
    let around = flat_length(&skin, Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0));
    let along = flat_length(&skin, Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
    assert!((around - FRAC_PI_2 * (r + K * T)).abs() < 1e-7, "{around} vs {}", FRAC_PI_2 * (r + K * T));
    assert!((along - 100.0).abs() < 1e-6, "{along}");
}

/// A strip 80 long bent along a cubic curve: developable, so it flattens with next to no stretch,
/// to the length of its neutral curve, measured here independently as a fine polyline of the
/// curve moved by the neutral surface's depth along its normal.
#[test]
fn a_bent_strip_flattens_to_its_neutral_curves_length() {
    let c = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 10.0), Vec2::new(80.0, 50.0), Vec2::new(90.0, 100.0)];
    let mut points = Vec::new();
    for p in c {
        for y in [0.0, 80.0] {
            points.push(Vec3::new(p.x, y, p.y));
        }
    }
    let skin = free(spline([1, 3], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]], &points, &[1.0; 8]));
    let dev = develop(&skin, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    assert!(flat.stretch < 1e-6, "{}", flat.stretch);
    // The curve in (x, z), its normal Su x Sv with Su along +y, and the neutral surface at K x T
    // or (1 - K) x T along it depending on which side the curve bends to.
    let at = |t: f64| {
        let s = 1.0 - t;
        let p = c[0] * (s * s * s) + c[1] * (3.0 * s * s * t) + c[2] * (3.0 * s * t * t) + c[3] * (t * t * t);
        let d = (c[1] - c[0]) * (3.0 * s * s) + (c[2] - c[1]) * (6.0 * s * t) + (c[3] - c[2]) * (3.0 * t * t);
        let dd = (c[2] - c[1] * 2.0 + c[0]) * (6.0 * s) + (c[3] - c[2] * 2.0 + c[1]) * (6.0 * t);
        // Su x Sv = (0, 1, 0) x (d.x, 0, d.y) = (d.y, 0, -d.x): in (x, z), (d.y, -d.x).
        let n = Vec2::new(d.y, -d.x) * (1.0 / d.len());
        let toward = dd.dot(n) > 0.0;
        p + n * if toward { (1.0 - K) * T } else { K * T }
    };
    let steps = 200_000;
    let expected: f64 = (0..steps).map(|k| at(k as f64 / steps as f64).dist(at((k + 1) as f64 / steps as f64))).sum();
    let got = flat_length(&skin, Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0));
    assert!((got - expected).abs() < 1e-7, "{got} vs {expected}");
}

/// A saddle is not developable: it still flattens, and says what that costs.
#[test]
fn a_saddle_flattens_and_reports_its_stretch() {
    let mut points = Vec::new();
    for v in [0.0, 1.0] {
        for u in [0.0, 1.0] {
            points.push(Vec3::new(u * 60.0, v * 60.0, if (u + v) == 1.0 { 15.0 } else { 0.0 }));
        }
    }
    let skin = free(spline([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], &points, &[1.0; 4]));
    let dev = develop(&skin, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    assert!(flat.stretch > 1e-3 && flat.stretch < 0.5, "{}", flat.stretch);
}

/// A straight line across a free-form panel's parameters flattens to exact Bezier pieces, one for
/// each knot span it crosses, of the spaces' degrees added: every point of the pieces is the
/// fitted map's point at the matching parameter.
#[test]
fn a_straight_trim_flattens_to_exact_bezier_pieces() {
    let c = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 10.0), Vec2::new(80.0, 50.0), Vec2::new(90.0, 100.0)];
    let mut points = Vec::new();
    for p in c {
        for y in [0.0, 80.0] {
            points.push(Vec3::new(p.x, y, p.y));
        }
    }
    let surface = spline([1, 3], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]], &points, &[1.0; 8]);
    let map = match crate::chart::Flatten::of(&surface, &Skin { thickness: T, k_factor: K, ..Skin::default() }) {
        Ok(m) => m,
        Err(e) => panic!("{e}"),
    };
    let (a, b) = (Vec2::new(0.1, 0.2), Vec2::new(0.9, 0.7));
    let line = Region2 { outer: Loop2 { segs: vec![Seg2::Line { a, b }] }, holes: Vec::new() };
    let Ok((flat, _)) = map.region(&line) else { panic!() };
    assert!(flat.outer.segs.iter().all(|s| matches!(s, Seg2::Bezier { n: 4, .. })), "{:?}", flat.outer.segs);
    // The pieces end to end follow the map along the line.
    let total: usize = flat.outer.segs.len();
    for (k, s) in flat.outer.segs.iter().enumerate() {
        for t in [0.0, 0.37, 1.0] {
            let p = s.point_at(t);
            let best = (0..=2000).map(|i| map.point(a.lerp(b, i as f64 / 2000.0)).dist(p)).fold(f64::INFINITY, f64::min);
            assert!(best < 0.05, "piece {k} of {total} at {t}: {best}");
        }
    }
}
