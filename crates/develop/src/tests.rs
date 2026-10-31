use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use solidcraft_geom::{Aabb2, Loop2, Region2, Seg2, Vec2, Vec3};

use crate::{
    Anchor, Bend, Corner, CornerRelief, Datum, DevelopError, Development, EdgeFrom, Joint, JointKind, Lap, Panel, Relief, Rigid2, Rigid3, Rip, Sheet,
    Surface, Tail, ZoneEnds, develop, fold,
};

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

/// The edges of panel `panel`'s flat outline that are joint `joint`'s rip.
fn rip_edges(dev: &Development, panel: usize, joint: usize) -> Vec<Seg2> {
    let Some(Some(flat)) = dev.panels.get(panel) else { return Vec::new() };
    let loops = std::iter::once(&flat.region.outer).chain(&flat.region.holes);
    loops.zip(&flat.origins).flat_map(|(l, from)| l.segs.iter().zip(from)).filter(|(_, o)| **o == EdgeFrom::Rip(joint)).map(|(s, _)| *s).collect()
}

/// A straight rip along a joint's anchors, `length` long, the sheet turning `turn` across it.
fn rip_along(joint: &Joint, length: f64, turn: f64, lap: Lap) -> JointKind {
    let line = |a: &Anchor| vec![a.origin, a.origin + a.dir * length];
    JointKind::Rip(Rip { edges: (line(&joint.a), line(&joint.b)), curves: (Vec::new(), Vec::new()), turns: vec![turn, turn], lap })
}

fn bend(angle: f64, datum: Datum, span: (f64, f64)) -> Bend {
    Bend {
        angle,
        radius: R,
        toward_material: true,
        datum,
        length: span.1,
        ends: ZoneEnds { near: span, far: span },
        tails: (Tail::Keep, Tail::Trim),
        relief: (Relief::None, Relief::None),
    }
}

/// An L bracket: a 100 x 60 base whose virtual sharp runs along y = 0, and a 20 mm flange drawn
/// in a chart of its own, turned and shifted so that nothing about its flat place is given
/// away. The flange's material lies on the right of its anchor.
fn bracket(b: Bend) -> Sheet {
    let chart = Rigid2::onto(Vec2::ZERO, Vec2::X, Vec2::new(7.0, -3.0), Vec2::from_angle(1.1)).unwrap_or(Rigid2::IDENTITY);
    Sheet {
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
        corners: Vec::new(),
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
    let sheet = bracket(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0)));
    let dev = develop(&sheet, 0).unwrap_or_default();
    let ba = FRAC_PI_2 * (R + K * T);
    let Some(Some(flange)) = dev.placements.get(1) else { panic!("the flange is placed: {dev:?}") };
    let Some(region) = sheet.panels.get(1).map(|p| flange.region(&p.region)) else { panic!() };
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
    let sheet = bracket(bend(1.2, Datum::Sharp, (10.0, 90.0)));
    let from_base = develop(&sheet, 0).unwrap_or_default();
    let from_flange = develop(&sheet, 1).unwrap_or_default();
    let (Some(Some(flange)), Some(Some(base))) = (from_base.placements.get(1), from_flange.placements.first()) else { panic!() };
    assert!(same_motion(base, &flange.inverse()));
}

/// A hem has no virtual sharp: anchored at its tangent lines, the flange's starts BA past the
/// base's. Anchored at a sharp it is refused.
#[test]
fn a_hem_is_anchored_at_its_tangent_lines() {
    let mut hem = bend(PI, Datum::Tangent, (0.0, 100.0));
    hem.radius = 0.01;
    let sheet = bracket(hem.clone());
    let dev = develop(&sheet, 0).unwrap_or_default();
    let ba = PI * (0.01 + K * T);
    let Some(Some(flange)) = dev.placements.get(1) else { panic!() };
    let Some(region) = sheet.panels.get(1).map(|p| flange.region(&p.region)) else { panic!() };
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
    let base_band = |relief: (Relief, Relief)| {
        let mut b = bend(FRAC_PI_2, Datum::Sharp, (5.0, 95.0));
        b.relief = relief;
        let dev = develop(&bracket(b), 0).unwrap_or_default();
        let bands: Vec<Aabb2> = dev.trims.iter().filter(|t| t.panel == 0).map(|t| bounds(&t.region)).collect();
        let [band] = bands.as_slice() else { panic!("one trim of the base: {:?}", dev.trims) };
        (band.min.x, band.max.x)
    };
    let (kept, relieved) = (base_band((Relief::None, Relief::None)), base_band((Relief::Notch, Relief::None)));
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
    let sheet = Sheet {
        thickness: T,
        k_factor: K,
        panels: (0..4).map(wall).collect(),
        joints: vec![joint(0, 1), joint(1, 2), joint(2, 3), joint(3, 0)],
        corners: Vec::new(),
    };
    let dev = develop(&sheet, 0).unwrap_or_default();
    assert!(dev.placements.iter().all(Option::is_some));
    let [miss] = dev.loops.as_slice() else { panic!("one loop: {:?}", dev.loops) };
    // Laid out from wall 0 both ways round, walls 1 and 2 go one way and wall 3 the other, so
    // the bend between walls 2 and 3 closes the ring: through it, wall 3 would lie four walls
    // and four bends from where the other way put it.
    let step = 40.0 + FRAC_PI_2 * (R + K * T) - 8.0;
    assert!(miss.joint == 2 && (miss.distance - 4.0 * step).abs() < 1e-9 && miss.angle < 1e-12, "{miss:?}");
    let ring_free = Sheet { joints: sheet.joints.iter().take(3).cloned().collect(), ..sheet.clone() };
    assert!(develop(&ring_free, 0).unwrap_or_default().loops.is_empty());
}

/// A rip is not followed: its panels are laid out as pieces of their own, the second beside the
/// first, and each loses its cut along the rip.
#[test]
fn a_rip_lays_its_panels_out_apart_and_bad_joints_are_refused() {
    let mut sheet = bracket(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0)));
    if let Some(j) = sheet.joints.get_mut(0) {
        j.kind = rip_along(j, 100.0, FRAC_PI_2, Lap::Cut(0.5, 0.25));
    }
    let dev = develop(&sheet, 0).unwrap_or_default();
    assert_eq!(dev.pieces, vec![vec![0], vec![1]]);
    assert!(dev.zones.is_empty() && dev.loops.is_empty());
    // The flange lies ten thicknesses to the right of the base, level with its lowest edge.
    let (Some(Some(place)), Some(flange)) = (dev.placements.get(1), sheet.panels.get(1)) else { panic!("the flange is laid out: {dev:?}") };
    let fb = bounds(&place.region(&flange.region));
    assert!((fb.min.x - 120.0).abs() < 1e-9 && fb.min.y.abs() < 1e-9, "{fb:?}");
    // The base loses 0.5 along the rip, into its material above y = 0: its edge there is the
    // rip's, and nothing is left to trim.
    assert!(dev.trims.is_empty(), "{:?}", dev.trims);
    let cut = rip_edges(&dev, 0, 0);
    let [Seg2::Line { a, b }] = cut.as_slice() else { panic!("one cut edge: {cut:?}") };
    assert!((a.y - 0.5).abs() < 1e-9 && (b.y - 0.5).abs() < 1e-9 && (a.x - b.x).abs() > 100.0 - 1e-9, "{a:?} {b:?}");
    let Some(Some(base)) = dev.panels.first() else { panic!("the base is laid out") };
    assert!((bounds(&base.region).min.y - 0.5).abs() < 1e-9, "{:?}", bounds(&base.region));
    assert_eq!(develop(&sheet, 5), Err(DevelopError::NoPanel(5)));
    if let Some(j) = sheet.joints.get_mut(0) {
        j.b.dir = Vec2::new(2.0, 0.0);
    }
    assert!(matches!(develop(&sheet, 0), Err(DevelopError::Joint(0, _))));
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
/// length, a quarter turn at K x T out from the inside of the roll. With the sheet on the inside
/// of the roll the neutral surface lies further out than the sheet; with it on the outside,
/// further in.
#[test]
fn a_roll_flattens_through_its_neutral_surface() {
    let r = 30.0;
    for (inward, neutral_radius) in [(false, r + K * T), (true, r - (1.0 - K) * T)] {
        let sheet = Sheet {
            thickness: T,
            k_factor: K,
            panels: vec![
                Panel { name: "floor".into(), region: rect(0.0, 0.0, 100.0, 40.0), ..Panel::default() },
                roll(r, inward, 100.0, FRAC_PI_2),
                Panel { name: "wall".into(), region: rect(0.0, 0.0, 100.0, 30.0), ..Panel::default() },
            ],
            joints: vec![seam(0, 40.0, 1, 0.0), seam(1, FRAC_PI_2, 2, 0.0)],
            corners: Vec::new(),
        };
        let dev = develop(&sheet, 0).unwrap_or_default();
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
    let sheet = Sheet {
        thickness: T,
        k_factor: K,
        panels: vec![Panel {
            name: "hopper".into(),
            surface: Surface::Cone { half_angle, material_inward: false },
            region: rect(40.0, 0.0, 90.0, turn),
        }],
        joints: Vec::new(),
        corners: Vec::new(),
    };
    let dev = develop(&sheet, 0).unwrap_or_default();
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
    let mut sheet = Sheet {
        thickness: T,
        k_factor: K,
        panels: vec![roll(r, true, 100.0, PI), roll(r, true, 100.0, PI)],
        joints: vec![seam(0, PI, 1, 0.0), seam(1, PI, 0, 0.0)],
        corners: Vec::new(),
    };
    assert_eq!(develop(&sheet, 0).unwrap_or_default().loops.len(), 1);
    if let Some(j) = sheet.joints.get_mut(1) {
        j.kind = rip_along(j, 100.0, 0.0, Lap::Cut(0.0, 0.0));
    }
    let dev = develop(&sheet, 0).unwrap_or_default();
    assert!(dev.loops.is_empty() && dev.pieces.len() == 1, "{dev:?}");
    let Some(Some(second)) = dev.panels.get(1) else { panic!() };
    let around = std::f64::consts::TAU * (r - (1.0 - K) * T);
    assert!((bounds(&second.region).max.y - around).abs() < 1e-9, "{:?}", bounds(&second.region));
}

/// A round hole on a roll, 10 mm across the sheet, is an ellipse in the roll's parameters (the
/// angle round it is length over the radius). Flattened, it lies on the neutral circle: still an
/// exact ellipse, as conics that each say which of the hole's segments they came from.
#[test]
fn a_hole_in_a_roll_flattens_to_an_exact_ellipse() {
    let (r, c, hole) = (40.0, Vec2::new(50.0, 30.0), 10.0);
    let on_surface = |p: Vec2| Vec2::new(p.x, p.y / r);
    let quarters: Vec<Seg2> = crate::chart::arc_as_conics(c, hole, 0.0, std::f64::consts::TAU)
        .into_iter()
        .map(|s| match s {
            Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: on_surface(a), apex: on_surface(apex), b: on_surface(b), w },
            other => other,
        })
        .collect();
    let mut panel = roll(r, false, 100.0, 60.0 / r);
    panel.region.holes.push(Loop2 { segs: quarters }.reversed());
    let sheet = Sheet { thickness: T, k_factor: K, panels: vec![panel], joints: Vec::new(), corners: Vec::new() };
    let dev = develop(&sheet, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    let (Some(ellipse), Some(from)) = (flat.region.holes.first(), flat.origins.get(1)) else { panic!("{flat:?}") };
    let k = (r + K * T) / r;
    assert!(ellipse.segs.iter().all(|s| matches!(s, Seg2::Conic { .. })), "{ellipse:?}");
    assert_eq!(from, &[0, 1, 2, 3].map(EdgeFrom::Own), "each quarter stays one conic");
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
    let mut sheet = bracket(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0)));
    if let Some(p) = sheet.panels.get_mut(1) {
        p.surface = Surface::Cylinder { radius: 50.0, material_inward: false };
    }
    assert!(matches!(develop(&sheet, 0), Err(DevelopError::Joint(0, _))));
}

fn spline(degree: [usize; 2], knots: [Vec<f64>; 2], points: &[Vec3], weights: &[f64]) -> Surface {
    match solidcraft_geom::bspline::NurbsSurface::new(degree, knots, points, weights) {
        Ok(s) => Surface::Spline(std::sync::Arc::new(s)),
        Err(e) => panic!("{e:?}"),
    }
}

/// A free-form panel over its whole parameter square.
fn free(surface: Surface) -> Sheet {
    Sheet {
        thickness: T,
        k_factor: K,
        panels: vec![Panel { name: "free".into(), surface, region: rect(0.0, 0.0, 1.0, 1.0) }],
        joints: Vec::new(),
        corners: Vec::new(),
    }
}

/// The flat length of the parameter line from `a` to `b`, along the flattened segments.
fn flat_length(sheet: &Sheet, a: Vec2, b: Vec2) -> f64 {
    let mut probe = sheet.clone();
    if let Some(p) = probe.panels.first_mut() {
        p.region = Region2 { outer: Loop2 { segs: vec![Seg2::Line { a, b }, Seg2::Line { a: b, b: a }] }, holes: Vec::new() };
    }
    let dev = develop(&probe, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    // Every piece the line came back as: one per knot span it crosses.
    let from = flat.origins.first().cloned().unwrap_or_default();
    flat.region.outer.segs.iter().zip(from).filter(|(_, o)| *o == EdgeFrom::Own(0)).map(|(s, _)| s.length()).sum()
}

/// A quarter of a cylinder of radius 40, 100 long, written as a rational spline (its angle is no
/// parameter of it), its sheet on the inside: fitted in its own spline space, it flattens to the
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
    let sheet = free(spline([1, 2], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]], &points, &weights));
    let dev = develop(&sheet, 0).unwrap_or_default();
    let Some(Some(flat)) = dev.panels.first() else { panic!("laid out: {dev:?}") };
    assert!(flat.stretch < 1e-6, "{}", flat.stretch);
    let around = flat_length(&sheet, Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0));
    let along = flat_length(&sheet, Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
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
    let sheet = free(spline([1, 3], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]], &points, &[1.0; 8]));
    let dev = develop(&sheet, 0).unwrap_or_default();
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
    let got = flat_length(&sheet, Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0));
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
    let sheet = free(spline([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], &points, &[1.0; 4]));
    let dev = develop(&sheet, 0).unwrap_or_default();
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
    let map = match crate::chart::Flatten::of(&surface, &Sheet { thickness: T, k_factor: K, ..Sheet::default() }) {
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

fn near3(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-9
}

/// Folded, the bracket's flange stands square on its base: turned toward the material it rises
/// on the material's side with its material toward the base, turned away it drops with its
/// material away from it. Its zone wraps from the base's tangent line to the flange's round an
/// axis the sheet's radius from both, and each zone edge the flat shares with a panel lands where
/// that panel's own does, at every depth.
#[test]
fn an_l_bracket_folds_square_round_its_bend() {
    for toward in [true, false] {
        let mut b = bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0));
        b.toward_material = toward;
        let sheet = bracket(b);
        let dev = develop(&sheet, 0).unwrap_or_default();
        let folded = fold(&sheet, &dev, &[Rigid3::IDENTITY]).unwrap_or_else(|e| panic!("{e}"));
        let side = if toward { 1.0 } else { -1.0 };
        // 30 along the sharp and 20 into the flange, from the `b` anchor.
        let Some(at) = sheet.joints.first().map(|j| j.b) else { panic!() };
        let q = at.origin + at.dir * 30.0 + Vec2::new(at.dir.y, -at.dir.x) * 20.0;
        for depth in [0.0, T] {
            let got = folded.surface_point(1, q, depth).unwrap_or_else(|| panic!("the flange is folded"));
            assert!(near3(got, Vec3::new(30.0, side * depth, side * 20.0)), "{toward}: {got:?} at {depth}");
        }
        let Some(Some(bent)) = folded.bends.first() else { panic!("the bend is folded") };
        let surface_r = if toward { R + T } else { R };
        let (axis_at, axis) = bent.axis();
        assert!(
            (axis_at.y - surface_r).abs() < 1e-9 && (axis_at.z - side * surface_r).abs() < 1e-9 && near3(axis, Vec3::X * -side),
            "{axis_at:?} {axis:?}"
        );
        let [zone] = dev.zones.as_slice() else { panic!("one zone") };
        for depth in [0.0, T] {
            for k in 0..2 {
                let (n, f) = (zone.near[k], zone.far[k]);
                let (Some(base), Some(flange)) = (folded.panel_point(0, n, depth), folded.panel_point(1, f, depth)) else { panic!() };
                assert!(near3(bent.point(n, depth), base) && near3(bent.point(f, depth), flange), "{toward}: zone end {k} at {depth}");
            }
            // Halfway round, the metal lies its radius at that depth from the axis.
            let (along, _) = bent.wrapped(zone.near[0]);
            let p = bent.at(along, FRAC_PI_4, depth);
            let rho = (p - axis_at) - axis * (p - axis_at).dot(axis);
            assert!((rho.len() - (surface_r - side * depth)).abs() < 1e-9, "{toward}: {rho:?} at {depth}");
        }
    }
}

/// Folded, a floor, a quarter roll and a wall joined by seams stand as drawn: the roll curls up
/// round an axis on the material's side when its material is inward, down when it is outward,
/// and the wall runs on square from its end. The seams' flat lines land as one at every depth.
#[test]
fn a_roll_folds_onto_its_cylinder_between_its_neighbours() {
    let r = 30.0;
    for (inward, rise) in [(true, 1.0), (false, -1.0)] {
        let sheet = Sheet {
            thickness: T,
            k_factor: K,
            panels: vec![
                Panel { name: "floor".into(), region: rect(0.0, 0.0, 100.0, 40.0), ..Panel::default() },
                roll(r, inward, 100.0, FRAC_PI_2),
                Panel { name: "wall".into(), region: rect(0.0, 0.0, 100.0, 30.0), ..Panel::default() },
            ],
            joints: vec![seam(0, 40.0, 1, 0.0), seam(1, FRAC_PI_2, 2, 0.0)],
            corners: Vec::new(),
        };
        let dev = develop(&sheet, 0).unwrap_or_default();
        let folded = fold(&sheet, &dev, &[Rigid3::IDENTITY]).unwrap_or_else(|e| panic!("{e}"));
        assert!(folded.loops.is_empty() && folded.bends.iter().all(Option::is_none));
        let (Some(Some(roll)), Some(Some(wall))) = (dev.panels.get(1), dev.panels.get(2)) else { panic!("laid out: {dev:?}") };
        let (seam_a, seam_b, top) = (40.0, bounds(&roll.region).max.y, bounds(&wall.region).max.y);
        for depth in [0.0, T] {
            let at = |i: usize, y: f64| folded.panel_point(i, Vec2::new(50.0, y), depth).unwrap_or_else(|| panic!("panel {i} is folded"));
            assert!(near3(at(0, seam_a), at(1, seam_a)) && near3(at(1, seam_b), at(2, seam_b)), "{inward}: seams at {depth}");
        }
        let got = folded.panel_point(2, Vec2::new(50.0, top), 0.0).unwrap_or_default();
        assert!(near3(got, Vec3::new(50.0, 40.0 + r, rise * (r + 30.0))), "{inward}: {got:?}");
        let Some((o, axis)) = folded.axis(1) else { panic!("the roll has an axis") };
        let p = folded.panel_point(1, Vec2::new(50.0, (seam_a + seam_b) / 2.0), 0.0).unwrap_or_default();
        assert!((((p - o) - axis * (p - o).dot(axis)).len() - r).abs() < 1e-9 && (axis.x.abs() - 1.0).abs() < 1e-12, "{inward}: {p:?}");
    }
}

/// A cone's flat sector maps back to the parameters it came from, whichever way round its angle
/// runs, and a depth into the material moves square off the cone by that much.
#[test]
fn a_cones_flat_maps_back_to_its_parameters() {
    let half_angle: f64 = 0.5;
    for inward in [true, false] {
        let surface = Surface::Cone { half_angle, material_inward: inward };
        let sheet = Sheet {
            thickness: T,
            k_factor: K,
            panels: vec![Panel { name: "hopper".into(), surface: surface.clone(), region: rect(40.0, 0.5, 90.0, 5.5) }],
            joints: Vec::new(),
            corners: Vec::new(),
        };
        let dev = develop(&sheet, 0).unwrap_or_default();
        let folded = fold(&sheet, &dev, &[Rigid3::IDENTITY]).unwrap_or_else(|e| panic!("{e}"));
        let (Ok(map), Some(Some(place))) = (crate::chart::Flatten::of(&surface, &sheet), dev.placements.first()) else { panic!() };
        for q in [Vec2::new(40.0, 0.5), Vec2::new(90.0, 5.5), Vec2::new(65.0, 3.0)] {
            let p = place.point(map.point(q));
            let back = folded.param(0, p).unwrap_or_default();
            assert!(near(back, q), "{inward}: {back:?} vs {q:?}");
            let (Some(surface_p), Some(deep)) = (folded.panel_point(0, p, 0.0), folded.panel_point(0, p, T)) else { panic!() };
            assert!((surface_p.len() - q.x).abs() < 1e-9 && (deep.dist(surface_p) - T).abs() < 1e-9, "{inward}: {surface_p:?} {deep:?}");
        }
    }
}

/// A square tube's four walls and four bends close a ring that the flat can't, but space can:
/// folded, the bend the layout left out fits where the other three put its walls. A fold needs
/// a place for each piece of the flat.
#[test]
fn a_ring_of_bends_closes_in_space() {
    let wall = |i: usize| Panel { name: format!("wall {i}"), region: rect(0.0, 0.0, 100.0, 40.0), ..Panel::default() };
    let joint = |a: usize, b: usize| Joint {
        a: Anchor { panel: a, origin: Vec2::new(100.0, 40.0), dir: Vec2::new(-1.0, 0.0) },
        b: Anchor { panel: b, origin: Vec2::new(100.0, 0.0), dir: Vec2::new(-1.0, 0.0) },
        kind: JointKind::Bend(bend(FRAC_PI_2, Datum::Sharp, (0.0, 100.0))),
    };
    let sheet = Sheet {
        thickness: T,
        k_factor: K,
        panels: (0..4).map(wall).collect(),
        joints: vec![joint(0, 1), joint(1, 2), joint(2, 3), joint(3, 0)],
        corners: Vec::new(),
    };
    let dev = develop(&sheet, 0).unwrap_or_default();
    let folded = fold(&sheet, &dev, &[Rigid3::IDENTITY]).unwrap_or_else(|e| panic!("{e}"));
    let [miss] = folded.loops.as_slice() else { panic!("one ring: {:?}", folded.loops) };
    assert!(miss.distance < 1e-9 && miss.angle < 1e-9, "{miss:?}");
    assert_eq!(fold(&sheet, &dev, &[]), Err(DevelopError::Roots(1)));
}

/// The base's trims for the bracket's bend other than its band along the zone: its reliefs.
fn reliefs(dev: &crate::Development) -> Vec<&Region2> {
    dev.trims.iter().filter(|t| t.panel == 0).skip(1).map(|t| &t.region).collect()
}

/// A floor 100 x 60 bent up along y = 0 and along x = 100, the bends meeting at the corner (100,
/// 0): their zones end where the floor's tangent lines cross, (96, 4) in the flat, and with
/// `flip` the second bend's joint runs the other way, from its wall to the floor.
fn corner(round: bool, flip: bool) -> Sheet {
    let ends = |near: (f64, f64)| ZoneEnds { near, far: near };
    let mut a = bend(FRAC_PI_2, Datum::Sharp, (0.0, 96.0));
    a.relief = (Relief::None, Relief::Notch);
    let mut b = bend(FRAC_PI_2, Datum::Sharp, (4.0, 60.0));
    b.relief = (Relief::Notch, Relief::None);
    let second = if flip {
        b.ends = ends((0.0, 56.0));
        b.relief = (Relief::None, Relief::Notch);
        Joint {
            a: Anchor { panel: 2, origin: Vec2::new(60.0, 0.0), dir: Vec2::new(-1.0, 0.0) },
            b: Anchor { panel: 0, origin: Vec2::new(100.0, 60.0), dir: Vec2::new(0.0, -1.0) },
            kind: JointKind::Bend(b),
        }
    } else {
        Joint {
            a: Anchor { panel: 0, origin: Vec2::new(100.0, 0.0), dir: Vec2::Y },
            b: Anchor { panel: 2, origin: Vec2::ZERO, dir: Vec2::X },
            kind: JointKind::Bend(b),
        }
    };
    Sheet {
        thickness: T,
        k_factor: K,
        panels: vec![
            Panel { name: "floor".into(), region: rect(0.0, 0.0, 100.0, 60.0), ..Panel::default() },
            Panel { name: "wall a".into(), region: rect(0.0, -30.0, 100.0, 0.0), ..Panel::default() },
            Panel { name: "wall b".into(), region: rect(0.0, -30.0, 60.0, 0.0), ..Panel::default() },
        ],
        joints: vec![
            Joint {
                a: Anchor { panel: 0, origin: Vec2::ZERO, dir: Vec2::X },
                b: Anchor { panel: 1, origin: Vec2::ZERO, dir: Vec2::X },
                kind: JointKind::Bend(a),
            },
            second,
        ],
        corners: vec![Corner { joints: [0, 1], relief: if round { CornerRelief::Round } else { CornerRelief::Square }, size: 8.0 }],
    }
}

/// A corner relief 8 across belongs to the corner, not to either panel: centered where the two
/// zones' center lines cross, BA / 2 out from the floor's tangent lines both ways, it is cut
/// from the flat wherever it reaches, whichever way the joints run.
#[test]
fn a_corner_relief_is_centered_where_the_bends_cross() {
    let ba = FRAC_PI_2 * (R + K * T);
    let center = Vec2::new(96.0 + ba / 2.0, 4.0 - ba / 2.0);
    for (round, area) in [(true, PI * 16.0), (false, 64.0)] {
        let cut = |flip: bool| {
            let dev = develop(&corner(round, flip), 0).unwrap_or_default();
            let [cut] = dev.cuts.as_slice() else { panic!("one cut: {:?}", dev.cuts) };
            cut.clone()
        };
        let (plain, flipped) = (cut(false), cut(true));
        for c in [&plain, &flipped] {
            let b = bounds(c);
            assert!((c.area() - area).abs() < 1e-9, "{round}: {}", c.area());
            assert!(b.min.dist(center - Vec2::new(4.0, 4.0)) < 1e-5 && b.max.dist(center + Vec2::new(4.0, 4.0)) < 1e-5, "{round}: {b:?}");
        }
    }
}

/// A bend relief: where the base runs on past the bend's end at x = 80, a slot 2 wide past it,
/// from 1 inside the base's tangent line (y = 4) out through the base's edge. At the end at
/// x = 20 the base runs on less far than a slot 30 wide, and none is cut.
#[test]
fn a_bend_relief_is_cut_where_the_base_runs_on_past_the_end() {
    let mut b = bend(FRAC_PI_2, Datum::Sharp, (20.0, 80.0));
    b.relief = (Relief::Slot { width: 30.0, depth: 1.0 }, Relief::Slot { width: 2.0, depth: 1.0 });
    let dev = develop(&bracket(b), 0).unwrap_or_default();
    let cuts = reliefs(&dev);
    let [slot] = cuts.as_slice() else { panic!("one relief: {:?}", dev.trims) };
    let sb = bounds(slot);
    assert!((sb.min.x - 80.0).abs() < 1e-9 && (sb.max.x - 82.0).abs() < 1e-9 && (sb.max.y - 5.0).abs() < 1e-9 && sb.min.y < 0.0, "{sb:?}");
}

/// A torn end follows its plane tight all the way round the bend. At the corner of the floor of
/// [`corner`], its first bend is torn against the plane halfway between the two walls (x + y =
/// 100), from the floor's corner, where its metal touches the plane, to the wall's end, a gap of
/// 0.5 cut off the plane: a couple of cubic spans in the flat that, folded, keep its metal,
/// through the whole thickness, off the plane by the gap's share for how far it has turned,
/// within a micron.
#[test]
fn a_torn_end_follows_its_plane_tight_round_the_bend() {
    let gap = 0.5;
    let wall_end = 100.0 - T - gap / std::f64::consts::SQRT_2;
    let mut sheet = corner(true, false);
    sheet.corners.clear();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    if let Some(JointKind::Bend(b)) = sheet.joints.first_mut().map(|j| &mut j.kind) {
        // The plane in the bend's frame at the floor's anchor: x along the joint, y up into the
        // material, z toward the wall (-y on the floor).
        b.relief = (Relief::None, Relief::Tear { normal: Vec3::new(-s, 0.0, s), offset: -100.0 * s });
        b.ends = ZoneEnds { near: (0.0, 96.0), far: (0.0, wall_end) };
    }
    let dev = develop(&sheet, 0).unwrap_or_default();
    let folded = fold(&sheet, &dev, &[Rigid3::IDENTITY]).unwrap_or_else(|e| panic!("{e}"));
    let (Some(zone), Some(Some(bent))) = (dev.zones.iter().find(|z| z.joint == 0), folded.bends.first()) else {
        panic!("the bend is laid out and folded")
    };
    let torn: Vec<&Seg2> = zone.region.outer.segs.iter().filter(|s| !matches!(s, Seg2::Line { .. })).collect();
    assert!(
        (1..=3).contains(&torn.len()) && torn.iter().all(|s| matches!(s, Seg2::Cubic { .. })),
        "a couple of cubic spans: {:?}",
        zone.region.outer.segs
    );
    let theta = bent.angle();
    let mut widest: f64 = 0.0;
    for seg in torn {
        for k in 0..=8 {
            let p = seg.point_at(k as f64 / 8.0);
            let (_, phi) = bent.wrapped(p);
            let off = |depth: f64| {
                let q = bent.point(p, depth);
                (100.0 - q.x - q.y) * s
            };
            let least = off(0.0).min(off(T));
            let want = gap / 2.0 * phi / theta;
            assert!((least - want).abs() < 1e-3, "at {phi}: {least} off the plane, not {want}");
            widest = widest.max(least);
        }
    }
    assert!((widest - gap / 2.0).abs() < 1e-9, "{widest}");
}

/// A mitered rip keeps each panel's metal, through its whole thickness, half the gap off the
/// plane halfway between them: at a square corner turning toward the material each is cut back
/// T + gap / sqrt 2, at one turning away gap / sqrt 2, and through a flat panel each side half the
/// gap. Turned either way the two sides swap.
#[test]
fn a_mitered_rip_keeps_both_panels_off_the_plane_between_them() {
    let gap = 0.5;
    let lap = Lap::Miter { gap };
    let close = |(a, b): (f64, f64), want: (f64, f64)| (a - want.0).abs() < 1e-12 && (b - want.1).abs() < 1e-12;
    let toward = T + gap / std::f64::consts::SQRT_2;
    assert!(close(lap.cuts(FRAC_PI_2, T), (toward, toward)), "{:?}", lap.cuts(FRAC_PI_2, T));
    let away = gap / std::f64::consts::SQRT_2;
    assert!(close(lap.cuts(-FRAC_PI_2, T), (away, away)), "{:?}", lap.cuts(-FRAC_PI_2, T));
    assert!(close(lap.cuts(0.0, T), (gap / 2.0, gap / 2.0)), "{:?}", lap.cuts(0.0, T));
    assert!(close(Lap::Cut(1.0, 2.0).cuts(0.3, T), (1.0, 2.0)));
}

/// A plate whose top is a half round of radius 27.5 about (0, 23), ripped from a roll along
/// that arc and cut back 3, its edge given at 33 points and, where `exact`, as the arc itself:
/// the plate's flat outline, each edge with what it is.
fn half_round_cut(exact: bool) -> Vec<(Seg2, EdgeFrom)> {
    let (r, c) = (27.5, Vec2::new(0.0, 23.0));
    let arc = Seg2::Arc { center: c, radius: r, start: 0.0, sweep: PI };
    let plate = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(-r, 23.0), b: Vec2::new(-r, -23.0) },
                Seg2::Line { a: Vec2::new(-r, -23.0), b: Vec2::new(r, -23.0) },
                Seg2::Line { a: Vec2::new(r, -23.0), b: Vec2::new(r, 23.0) },
                arc,
            ],
        }
        .ccw(),
        holes: Vec::new(),
    };
    let n = 33;
    let along: Vec<Vec2> = (0..n).map(|k| arc.point_at(k as f64 / (n - 1) as f64)).collect();
    let round: Vec<Vec2> = (0..n).map(|k| Vec2::new(0.0, PI * k as f64 / (n - 1) as f64)).collect();
    let curves = (if exact { vec![arc] } else { Vec::new() }, Vec::new());
    let sheet = Sheet {
        thickness: T,
        k_factor: K,
        panels: vec![Panel { name: "plate".into(), region: plate, ..Panel::default() }, roll(r, true, 36.0, PI)],
        joints: vec![Joint {
            a: Anchor { panel: 0, origin: Vec2::new(r, 23.0), dir: Vec2::new(-1.0, 0.0) },
            b: Anchor { panel: 1, origin: Vec2::ZERO, dir: Vec2::Y },
            kind: JointKind::Rip(Rip { edges: (along, round), curves, turns: vec![-FRAC_PI_2; n], lap: Lap::Cut(3.0, 0.0) }),
        }],
        corners: Vec::new(),
    };
    let dev = develop(&sheet, 0).unwrap_or_default();
    assert!(dev.trims.is_empty(), "{:?}", dev.trims);
    let Some(Some(plate)) = dev.panels.first() else { panic!("the plate is laid out: {dev:?}") };
    let from = plate.origins.first().cloned().unwrap_or_default();
    plate.region.outer.segs.iter().copied().zip(from).collect()
}

/// A rip follows its edge wherever it runs: given only as points along the half round, the
/// plate's new edge is a spline through the cut points within a micron of the arc 3 inside.
#[test]
fn a_rip_follows_a_curved_edge() {
    let (r, c) = (27.5, Vec2::new(0.0, 23.0));
    let cut = half_round_cut(false);
    let curved: Vec<&Seg2> = cut.iter().filter(|(s, o)| matches!(s, Seg2::Cubic { .. }) && *o == EdgeFrom::Rip(0)).map(|(s, _)| s).collect();
    assert_eq!(curved.len(), 32);
    for s in curved {
        for k in 0..=8 {
            let d = s.point_at(k as f64 / 8.0).dist(c);
            assert!((d - (r - 3.0)).abs() < 1e-3, "{d}");
        }
    }
}

/// Where the edge is known exactly, an even cut moves it exactly: the half round cut back 3 is
/// one arc, 3 inside it about the same center. The plate's sides, not ripped, keep their whole
/// length: the cut steps square in from each to the arc, and both steps are the rip's. Nothing
/// else is cut, and no edge is in pieces.
#[test]
fn a_rip_along_a_known_arc_cuts_an_arc() {
    let (r, c) = (27.5, Vec2::new(0.0, 23.0));
    let cut = half_round_cut(true);
    let rip: Vec<&Seg2> = cut.iter().filter(|(_, o)| *o == EdgeFrom::Rip(0)).map(|(s, _)| s).collect();
    let arcs: Vec<&&Seg2> = rip.iter().filter(|s| !matches!(s, Seg2::Line { .. })).collect();
    let [Seg2::Arc { center, radius, sweep, .. }] = arcs.as_slice() else { panic!("one arc: {rip:?}") };
    assert!(center.dist(c) < 1e-12 && (radius - (r - 3.0)).abs() < 1e-12 && (sweep.abs() - PI).abs() < 1e-12, "{rip:?}");
    let steps = rip
        .iter()
        .filter(|s| matches!(s, Seg2::Line { a, b } if (a.y - 23.0).abs() < 1e-12 && (b.y - 23.0).abs() < 1e-12 && (a.dist(*b) - 3.0).abs() < 1e-12));
    assert_eq!(steps.count(), 2, "{rip:?}");
    let own: Vec<&Seg2> = cut.iter().filter(|(_, o)| matches!(o, EdgeFrom::Own(_))).map(|(s, _)| s).collect();
    assert_eq!(own.len(), 3, "the plate's sides and bottom, whole: {own:?}");
    assert!(own.iter().all(|s| (s.length() - 46.0).abs() < 1e-12 || (s.length() - 2.0 * r).abs() < 1e-12), "{own:?}");
}

/// On a roll the cut is measured on the roll: a rip along its straight edge, cut back 2, takes
/// 2 / r of a turn off it, which lies 2 x (neutral radius) / r into its flat.
#[test]
fn a_rip_on_a_roll_is_cut_back_on_the_roll() {
    let r = 30.0;
    let mut sheet = Sheet {
        thickness: T,
        k_factor: K,
        panels: vec![roll(r, true, 100.0, FRAC_PI_2), Panel { name: "wall".into(), region: rect(0.0, 0.0, 100.0, 30.0), ..Panel::default() }],
        joints: vec![seam(0, FRAC_PI_2, 1, 0.0)],
        corners: Vec::new(),
    };
    if let Some(j) = sheet.joints.get_mut(0) {
        j.kind = rip_along(j, 100.0, 0.0, Lap::Cut(2.0, 0.0));
    }
    let dev = develop(&sheet, 0).unwrap_or_default();
    let neutral = r - (1.0 - K) * T;
    let want = (FRAC_PI_2 - 2.0 / r) * neutral;
    let cut = rip_edges(&dev, 0, 0);
    let [Seg2::Line { a, b }] = cut.as_slice() else { panic!("one cut edge: {cut:?}") };
    assert!((a.y - want).abs() < 1e-6 && (b.y - want).abs() < 1e-6, "{a:?} {b:?} vs {want}");
}
