//! Shells, face offsets, thickening and sheet offsets against closed forms (the cases of
//! SolidCraft's `core/offset_tests.rs`, on record face indices through the body's `Order`).

use std::f64::consts::PI;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::{edge_closest_point, face_bounding_box};

use super::{offset_faces, offset_sheet, shell, thicken};
use crate::blend::{blend, Blend, Corners};
use crate::session::{self, run};
use crate::{make_box, make_cylinder, make_sphere, Body, Error, Order};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

/// The 30 x 20 x 10 block at the origin.
fn block() -> Body {
    make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap()
}

const ABC: f64 = 6000.0;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1.0)
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

/// The record order of `b`: record face `i` is the kernel face `order.faces[i]`.
fn order(b: &Body) -> Order {
    b.to_record().unwrap().1
}

/// The record index (through `order`) of the face of `b` whose tight box `pick` accepts.
fn face(b: &Body, order: &Order, pick: impl Fn(&Extent3d) -> bool) -> usize {
    let k = Kernel::default();
    let brep = b.lock();
    order
        .faces
        .iter()
        .position(|&f| pick(&face_bounding_box(&k, &brep, f, true).unwrap()))
        .unwrap()
}

/// The block's top, `z = 10`, by its record index.
fn top(b: &Body, order: &Order) -> usize {
    face(b, order, |bx| {
        (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9
    })
}

#[test]
fn a_block_shells_to_its_closed_forms() {
    all_fixes();
    // inward 2, open at the top: abc - (a - 4)(b - 4)(c - 2)
    let b = block();
    let o = order(&b);
    shell(&b, &o, &[top(&b, &o)], -2.0).unwrap();
    assert!(rel(volume(&b), ABC - 26.0 * 16.0 * 8.0) < 1e-9);
    // closed: (a - 4)(b - 4)(c - 4) hollow
    let b = block();
    shell(&b, &order(&b), &[], -2.0).unwrap();
    assert!(rel(volume(&b), ABC - 26.0 * 16.0 * 6.0) < 1e-9);
    // outward 1, the convex edges extended: (a + 2t)(b + 2t)(c + t) - abc
    let b = block();
    let o = order(&b);
    shell(&b, &o, &[top(&b, &o)], 1.0).unwrap();
    assert!(rel(volume(&b), 32.0 * 22.0 * 11.0 - ABC) < 1e-9);
}

/// A shell keeps its rounds by their definitions. The shell's working copy is the body
/// itself, definitions included, so a round's offset is the round of the moved radius on the
/// offset supports (`Surface::create_offset_surface`, solidcore #2487), not a generic offset of
/// its net. A cylinder r 20, z -20..20 with its bottom rim rounded r 10.3, shelled 2 in and
/// open at the top: both round faces stay blends, the outer one r 10.3 and the cavity's r 8.3.
#[test]
fn a_shell_keeps_its_rounds_as_blends_of_the_moved_radius() {
    all_fixes();
    let body = make_cylinder([0.0, 0.0, -20.0], 20.0, 40.0).unwrap();
    // the bottom rim, away from the cylinder's seam at x = 20, y = 0
    let rim = edge_at(&body, [0.0, 20.0, -20.0]);
    blend(&body, Blend::Round, &[rim], &[10.3], &[], Corners::Filled).unwrap();
    let o = order(&body);
    let open = face(&body, &o, |bx| {
        (bx.min().z - 20.0).abs() < 1e-9 && (bx.max().z - 20.0).abs() < 1e-9
    });
    shell(&body, &o, &[open], -2.0).unwrap();
    let (record, _) = body.to_record().unwrap();
    let kinds: Vec<&str> = record
        .faces
        .iter()
        .map(|f| match &f.surface {
            solidcore_record::brep::Surface::Plane { .. } => "plane",
            solidcore_record::brep::Surface::Cylinder { .. } => "cylinder",
            solidcore_record::brep::Surface::Torus { .. } => "torus",
            solidcore_record::brep::Surface::Spline { .. } => "spline",
            solidcore_record::brep::Surface::RationalSpline { .. } => "rational_spline",
            solidcore_record::brep::Surface::Blend { .. } => "blend",
            _ => "other",
        })
        .collect();
    let mut radii = Vec::new();
    for f in &record.faces {
        let solidcore_record::brep::Surface::Blend { definition } = &f.surface else {
            continue;
        };
        radii.push(match definition.as_ref() {
            solidcore_record::brep::blend::BlendDefinition::Procedural(d) => match &d.radius {
                solidcore_record::brep::blend::RadiusRecord::Constant { offsets } => {
                    offsets[0].abs()
                }
                solidcore_record::brep::blend::RadiusRecord::Law(_)
                | solidcore_record::brep::blend::RadiusRecord::Edge
                | solidcore_record::brep::blend::RadiusRecord::EqualSetback => continue,
            },
            solidcore_record::brep::blend::BlendDefinition::ExactRound(d) => d.radius,
        });
    }
    radii.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        radii.len(),
        2,
        "round faces kept as blends, radii {radii:?}; face kinds {kinds:?}"
    );
    assert!(
        (radii[0] - 8.3).abs() < 1e-9,
        "cavity round radius {} for 8.3",
        radii[0]
    );
    assert!(
        (radii[1] - 10.3).abs() < 1e-9,
        "outer round radius {} for 10.3",
        radii[1]
    );
}

#[test]
fn a_cylinder_shells_open_at_its_top() {
    all_fixes();
    // r 5, h 10, t 1: pi r^2 h - pi (r - t)^2 (h - t)
    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let o = order(&c);
    let at = face(&c, &o, |bx| {
        (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9
    });
    shell(&c, &o, &[at], -1.0).unwrap();
    assert!(rel(volume(&c), PI * 25.0 * 10.0 - PI * 16.0 * 9.0) < 1e-9);
}

fn drilled_half_torus(center: f64) -> Body {
    let torus = crate::make_torus([0.0; 3], 30.0, 8.0).unwrap();
    let upper = make_box([-40.0, -40.0, 0.0], [80.0, 80.0, 16.0]).unwrap();
    let half = crate::boolean::boolean(torus, upper, crate::BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    let drill = make_cylinder([center, 0.0, 0.0], 0.95, 4.0).unwrap();
    crate::boolean::boolean(half, drill, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// Stress corpus S1983: the blind hole leaves 0.00002 mm at the outer rim of a
/// half torus. Opening the annular bottom must produce a closed hollow body.
#[test]
fn a_half_torus_with_a_blind_hole_shells_open_at_its_bottom() {
    crate::test_corpus::rig_fixes();
    let body = drilled_half_torus(37.049_98);
    let input_report = body.copy().unwrap().check(crate::CheckLevel::All).unwrap();
    assert!(input_report.is_valid(), "{}", input_report.text);
    let o = order(&body);
    let bottom = face(&body, &o, |bx| {
        bx.min().z.abs() < 1e-9 && bx.max().z.abs() < 1e-9
    });
    shell(&body, &o, &[bottom], -0.8).unwrap();
    let report = body.check(crate::CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // Half-torus wall volume minus the original drill/torus overlap, plus the
    // expanded drill/cavity overlap; the polar integral is derived in the finding.
    let actual = volume(&body);
    assert!(
        rel(actual, 3_607.142_961_425_14) < 1e-9,
        "shell volume {actual:.17}"
    );
}

/// `f` integrated over `[a, b]` by 5-point Gauss-Legendre on 400 pieces, after
/// `x = a + (b - a) (3 u^2 - 2 u^3)`, which makes an integrand vanishing as a power 3/2 at
/// either end (a lens opening from, or closing to, a point) smooth in `u`.
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn integral(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let inner = (5.0 - 2.0 * (10.0_f64 / 7.0).sqrt()).sqrt() / 3.0;
    let outer = (5.0 + 2.0 * (10.0_f64 / 7.0).sqrt()).sqrt() / 3.0;
    let w_inner = (322.0 + 13.0 * 70.0_f64.sqrt()) / 900.0;
    let w_outer = (322.0 - 13.0 * 70.0_f64.sqrt()) / 900.0;
    let nodes = [
        (0.0, 128.0 / 225.0),
        (inner, w_inner),
        (-inner, w_inner),
        (outer, w_outer),
        (-outer, w_outer),
    ];
    let pieces = 400;
    let h = 1.0 / f64::from(pieces);
    let mut sum = 0.0;
    for i in 0..pieces {
        let mid = (f64::from(i) + 0.5) * h;
        for (x, w) in nodes {
            let u = mid + 0.5 * h * x;
            let at = a + (b - a) * u * u * (3.0 - 2.0 * u);
            sum += 0.5 * h * w * f(at) * (b - a) * 6.0 * u * (1.0 - u);
        }
    }
    sum
}

/// The area of the disc of radius `r` beyond a chord `d` from its centre (none when `d >= r`).
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn disc_segment(r: f64, d: f64) -> f64 {
    if d >= r {
        return 0.0;
    }
    r * r * (d / r).acos() - d * (r * r - d * d).sqrt()
}

/// The area of the rectangle `|x| <= a`, `|y| <= b` inside the disc of radius `r` about its
/// centre: the disc less its segments beyond the four sides, each corner's part beyond two
/// sides taken twice and given back, `F(x) = (x sqrt(r^2 - x^2) + r^2 asin(x / r)) / 2` the
/// area under the circle from 0 to x.
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn rectangle_in_disc(r: f64, a: f64, b: f64) -> f64 {
    let under = |x: f64| 0.5 * (x * (r * r - x * x).sqrt() + r * r * (x / r).asin());
    let corner = if r * r > a * a + b * b {
        let reach = (r * r - b * b).sqrt();
        under(reach) - under(a) - b * (reach - a)
    } else {
        0.0
    };
    PI * r * r - 2.0 * disc_segment(r, a) - 2.0 * disc_segment(r, b) + 4.0 * corner
}

/// A 40 x 38 x 100 block (|x| <= 20, |y| <= 19, 0 <= z <= 100) cut by the ball of r 55 about
/// (0, 0, 50), which takes its eight vertical corners at the bottom and the top, with the
/// sixteen edges where the sphere meets the sides rounded r 1, shelled 1 in and open at the
/// top. Each round collapses onto its spine (an inset by its radius), and the faces beside it
/// meet across it. At each corner the two rounds meet each other along a mitre, and the block
/// not being square, a round there also touches the side beyond its own on a sliver: a round of
/// five edges, whose rails are its two edges tangent to the sphere and its side (not opposite
/// in a loop of four, as a lone round's are).
///
/// Every rounded edge is convex and the wall is the rounds' radius, so the cavity is the inset
/// of the sharp body: the points with |x| < 19, |y| < 18 and z > 1 inside the ball of r 54,
/// up to the open top. The shell takes that much away: the integral over z in (1, 100) of the
/// area of the 38 x 36 rectangle inside the circle of radius sqrt(54^2 - (z - 50)^2)
/// (`rectangle_in_disc`), split where the circle passes the rectangle's corners.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle and edge picks, not ops geometry
fn an_inset_by_the_rounds_radius_mates_across_rounds_that_meet_at_a_corner() {
    crate::test_corpus::rig_fixes();
    let block = make_box([-20.0, -19.0, 0.0], [40.0, 38.0, 100.0]).unwrap();
    let ball = make_sphere([0.0, 0.0, 50.0], 55.0).unwrap();
    let body = crate::boolean::boolean(block, ball, crate::BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    // a point of each sphere-to-side edge, on x = +-20 or y = +-19, at the bottom and the top
    let mut edges = Vec::new();
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        for [x, y] in [[20.0 * sx, 17.0 * sy], [18.0 * sx, 19.0 * sy]] {
            let h = (55.0_f64 * 55.0 - x * x - y * y).sqrt();
            edges.push(edge_at(&body, [x, y, 50.0 - h]));
            edges.push(edge_at(&body, [x, y, 50.0 + h]));
        }
    }
    blend(
        &body,
        Blend::Round,
        &edges,
        &[1.0; 16],
        &[],
        Corners::Filled,
    )
    .unwrap();
    let before = volume(&body);
    let o = order(&body);
    let open = face(&body, &o, |bx| {
        (bx.min().z - 100.0).abs() < 1e-9 && (bx.max().z - 100.0).abs() < 1e-9
    });
    shell(&body, &o, &[open], -1.0).unwrap();
    let report = body.check(crate::CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let section = |z: f64| {
        rectangle_in_disc(
            (54.0_f64 * 54.0 - (z - 50.0) * (z - 50.0)).sqrt(),
            19.0,
            18.0,
        )
    };
    // the circle passes the rectangle's corners, sqrt(19^2 + 18^2) out, at z = 50 -+ sqrt(54^2 - 685)
    let corners = (54.0_f64 * 54.0 - 685.0).sqrt();
    let cavity = integral(1.0, 50.0 - corners, section)
        + integral(50.0 - corners, 50.0 + corners, section)
        + integral(50.0 + corners, 100.0, section);
    let removed = before - volume(&body);
    assert!(
        rel(removed, cavity) < 1e-9,
        "the shell took {removed:.17} away, the cavity is {cavity:.17}"
    );
}

/// The area two discs of radii `big` and `r`, centres `d` apart, share.
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn discs_overlap(big: f64, r: f64, d: f64) -> f64 {
    if d >= big + r {
        return 0.0;
    }
    if d <= (big - r).abs() {
        return PI * big.min(r).powi(2);
    }
    let a = r * r * ((d * d + r * r - big * big) / (2.0 * d * r)).acos();
    let b = big * big * ((d * d + big * big - r * r) / (2.0 * d * big)).acos();
    let c = 0.5 * ((-d + r + big) * (d + r - big) * (d - r + big) * (d + r + big)).sqrt();
    a + b - c
}

/// A frustum about +Z, r 100 at z = 0 to r 100 - 200 tan 18° at z = 200 (its side 18° off the
/// axis), with a hole of r 5 from z = 150 up through the top, its axis 32 out at 85° from +X:
/// it reaches r 37, past the rim's 35.02, so it breaks out through the side near the top. The
/// hole's wall is one periodic face whose seam, at +X from its axis, is 85° round from the
/// hole's outermost line. Shelled 2 in, open at the bottom.
///
/// The edge where the side meets the hole's wall is a U from the top down to z ~ 194 and back.
/// Its offset, where the side's offset meets the wall's (r 7), runs from the top's offset
/// (z = 198) at 83° either way round the hole from its outermost line, down to z ~ 181; on the
/// extended surfaces it goes on up past the top's offset and crosses the wall's seam at 85°,
/// z ~ 198.7. The seam cuts the intersection there into the whole U and a short piece beyond the
/// seam, above the top's offset, whose middle lies nearer the edge's middle than the U's. No
/// neighbour's offset cuts that piece: its ends are the edge's vertices dropped onto it. Taken
/// for the edge, it left the loops of the side's and the wall's offsets open (`LoopGapUncertified`).
///
/// Every edge but the open bottom's moves with its faces, so the cavity is bounded by the
/// faces' offsets alone: inside the side's offset, r 100 - z tan 18° - 2 / cos 18°, from the
/// bottom up to z = 198, less, from z = 148, the r 7 disc 32 off the axis. The shell takes
/// away the integral over z of that section's area.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle and face picks, not ops geometry
fn a_cones_side_broken_through_by_a_hole_past_its_seam_shells_to_its_closed_form() {
    crate::test_corpus::rig_fixes();
    let t18 = 18.0_f64.to_radians().tan();
    let top = 100.0 - 200.0 * t18;
    let (c, s) = (85.0_f64.to_radians().cos(), 85.0_f64.to_radians().sin());
    let cone = crate::make_cone([0.0; 3], 100.0, top, 200.0).unwrap();
    let hole = make_cylinder([32.0 * c, 32.0 * s, 150.0], 5.0, 51.0).unwrap();
    let body = crate::boolean::boolean(cone, hole, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let before = volume(&body);
    let o = order(&body);
    let bottom = face(&body, &o, |bx| {
        bx.min().z.abs() < 1e-9 && bx.max().z.abs() < 1e-9
    });
    shell(&body, &o, &[bottom], -2.0).unwrap();
    let report = body.check(crate::CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let inside = |z: f64| 100.0 - z * t18 - 2.0 / 18.0_f64.to_radians().cos();
    // the r 7 disc leaves the side's offset where 32 + 7 = inside(z), z ~ 181.3: split there
    let out = (100.0 - 2.0 / 18.0_f64.to_radians().cos() - 39.0) / t18;
    let section = |z: f64| PI * inside(z).powi(2);
    let notch = |z: f64| discs_overlap(inside(z), 7.0, 32.0);
    let cavity =
        integral(0.0, 198.0, section) - integral(148.0, out, notch) - integral(out, 198.0, notch);
    let removed = before - volume(&body);
    assert!(
        rel(removed, cavity) < 1e-9,
        "the shell took {removed:.17} away, the cavity is {cavity:.17}"
    );
}

/// A frustum on the z axis, r 10 at z = 0 to r 4 at z = 20 (its side falls 0.3 per mm), with
/// everything at y > 6.8 cut away from below up to z = 10: a slot whose wall, the plane
/// y = 6.8, runs along the frustum's side. The slot's ceiling, z = 10, is a lens of two edges
/// between the wall's line and the side's circle (r 7 there): 0.2 across at x = 0, its two
/// vertices the only places the side, the wall and the ceiling meet. The side meets the wall
/// along a hyperbola.
fn slotted_frustum() -> Body {
    let frustum = crate::make_cone([0.0; 3], 10.0, 4.0, 20.0).unwrap();
    let slot = make_box([-20.0, 6.8, -1.0], [40.0, 20.0, 11.0]).unwrap();
    crate::boolean::boolean(frustum, slot, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// acid-1's notch ceiling where it breaks out through the torus, on a frustum: the slotted
/// frustum shelled closed. The void is the core: the frustum with its side moved in t along
/// its normal (its radius at z is 10 - 0.3 z - t sqrt(1.09)) between z = t and 20 - t, less the
/// slot grown by t (its wall at y = 6.8 - t, its ceiling at 10 + t). At t = 1 the side and the
/// wall no longer meet on the ceiling's plane, z = 11 (the side is there at 10 - 3.3 - 1.044 =
/// 5.656, the wall at 5.8): the lens's offset is empty (the offset eliminates it), and the
/// wall meets the side directly up to z* = (10 - 1.044 - 5.8) / 0.3 = 10.52, where they part.
/// At t = 0.1 the lens's offset is still there (the wall at 6.7 against the side at 6.866 on
/// z = 10.1). The shell is the body less the core, each a frustum less its slot: at each
/// height z the slot takes the disc's segment beyond the wall (`disc_segment`), up to the
/// ceiling or to where the side and the wall part, whichever comes first.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_lens_whose_offset_is_empty_leaves_the_shell() {
    crate::test_corpus::rig_fixes();
    // the frustum moved in by `t` less its slot moved out by `t`
    let core = |t: f64| {
        let radius = |z: f64| 10.0 - 0.3 * z - t * 1.09_f64.sqrt();
        let (bottom, top) = (radius(t), radius(20.0 - t));
        let wall = 6.8 - t;
        let parts = (10.0 - t * 1.09_f64.sqrt() - wall) / 0.3;
        PI / 3.0 * (20.0 - 2.0 * t) * (bottom * bottom + bottom * top + top * top)
            - integral(t, (10.0 + t).min(parts), |z| disc_segment(radius(z), wall))
    };
    let body_volume = core(0.0);
    for t in [1.0, 0.1] {
        let body = slotted_frustum();
        assert!(
            rel(volume(&body), body_volume) < 1e-9,
            "body {}",
            volume(&body)
        );
        shell(&body, &order(&body), &[], -t).unwrap_or_else(|e| panic!("t {t}: {e:?}"));
        let report = body.check(crate::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "t {t}: {}", report.text);
        let want = body_volume - core(t);
        let actual = volume(&body);
        assert!(
            rel(actual, want) < 1e-9,
            "t {t}: shell volume {actual:.17}, want {want:.17}"
        );
    }
}

/// The same incidence graph before the seam disappears: the outer rim is
/// 37.99 and the grown drill reaches 37.96, so their meridian still survives.
#[test]
fn a_half_torus_keeps_a_periodic_seam_when_its_offsets_still_meet() {
    crate::test_corpus::rig_fixes();
    let body = drilled_half_torus(37.0);
    let o = order(&body);
    let bottom = face(&body, &o, |bx| {
        bx.min().z.abs() < 1e-9 && bx.max().z.abs() < 1e-9
    });
    shell(&body, &o, &[bottom], -0.01).unwrap();
    let report = body.check(crate::CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // The same polar integral as S1983, with a=37 and wall=0.01;
    // its hand derivation and converged values are in the construction finding.
    let actual = volume(&body);
    assert!(
        rel(actual, 47.498_020_998_403_7) < 1e-9,
        "shell volume {actual:.17}"
    );
    let (record, _) = body.to_record().unwrap();
    let cavity_faces: Vec<_> = record
        .faces
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            matches!(
                f.surface,
                solidcore_record::brep::Surface::Torus { minor_radius, .. } if minor_radius < 8.0
            )
            .then_some(i)
        })
        .collect();
    assert_eq!(cavity_faces.len(), 1, "one inset torus face");
    let cavity = cavity_faces[0];
    let query = crate::query::Query::new(&record);
    // The short seam has two distinct ends beyond the drill centre, x = 37.
    // The other meridian ends on the cap at x < 37; the outer rim is closed.
    let seam_edges: Vec<_> = record
        .edges
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            query.edge_faces(*i).contains(&cavity)
                && e.vertices[0] != e.vertices[1]
                && e.vertices
                    .iter()
                    .all(|&v| record.vertices[v as usize].point[0] > 37.0)
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(seam_edges.len(), 1, "one retained outer-rim seam");
    assert_eq!(query.edge_faces(seam_edges[0]), [cavity, cavity]);
}

/// A 30 x 30 x 20 block with a conical recess from its bottom (radius 7.2 at z = 0, falling
/// 0.4 per mm), in which a 12 x 12 post stands from z = 1 up: the post's sides poke into the
/// recess between z = 1 and z = 3, each through a lens whose hyperbola has its apex on the
/// cone's seam.
fn block_with_a_post_in_a_conical_recess() -> Body {
    let block = make_box([-15.0, -15.0, 0.0], [30.0, 30.0, 20.0]).unwrap();
    let cone = crate::make_cone([0.0; 3], 7.2, 3.2, 10.0).unwrap();
    let post = make_box([-6.0, -6.0, 1.0], [12.0, 12.0, 20.0]).unwrap();
    let recess = crate::boolean::boolean(cone, post, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    crate::boolean::boolean(block, recess, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// #19714: shelled open at the post's bottom, the cone's seam still ends on each lens's
/// hyperbola. The piece of the hyperbola beyond the open face, down to the recess's rim, is
/// not the edge's image, so the seam is kept and the body is hollow.
///
/// The hollow is the body less its core. With no change of topology the core is the same
/// solid with every face's surface moved t into the material and the open face left in
/// place: the block inset by t, less the cone r(z) = 7.2 + t sqrt(1.16) - 0.4 z outside the
/// post of half side 6 - t from z = 1. Its volume is a frustum plus the integral, from z = 1
/// to where r(z) = 6 - t, of the disc's area outside the square,
/// 4 (r^2 acos(s / r) - s sqrt(r^2 - s^2)); the values are in the finding
/// `offset.offset_geometry_creation--periodic-end-leaves-far-support-19714.md`.
#[test]
fn a_post_in_a_conical_recess_shells_open_at_its_bottom() {
    crate::test_corpus::rig_fixes();
    let whole = 17_835.098_082_931_83;
    for (t, hollow) in [(0.4, 1_631.138_823_196_25), (0.2, 822.872_121_654_74)] {
        let body = block_with_a_post_in_a_conical_recess();
        assert!(rel(volume(&body), whole) < 1e-9, "body {}", volume(&body));
        let o = order(&body);
        let bottom = face(&body, &o, |bx| {
            (bx.min().z - 1.0).abs() < 1e-9 && (bx.max().z - 1.0).abs() < 1e-9
        });
        shell(&body, &o, &[bottom], -t).unwrap_or_else(|e| panic!("t {t}: {e:?}"));
        let report = body.check(crate::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "t {t}: {}", report.text);
        let actual = volume(&body);
        assert!(
            rel(actual, hollow) < 1e-9,
            "t {t}: shell volume {actual:.17}"
        );
    }
}

/// The Dutchman stress corpus's Shell family (S0003, S0012, S0061): a feature no wider than
/// twice the wall holds no cavity. A cylinder r 1.5, h 4 with a
/// pin r 0.15, 1.2 high on its top, and a 30 x 20 x 10 block with a 2 x 2 x 3 boss on its
/// top, each shelled closed with a wall wider than half the feature (0.18; 1.2). The pin's
/// side's offset collapses and the boss's walls meet across it, which refused the shell; the
/// hollow is the body's without the feature, the feature left solid on it.
///
/// Closed forms: pi 1.5^2 4 + pi 0.15^2 1.2 - pi (1.5 - 0.18)^2 (4 - 0.36), and
/// 6000 + 12 - (30 - 2.4)(20 - 2.4)(10 - 2.4).
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_feature_narrower_than_twice_the_wall_stays_solid() {
    all_fixes();
    let base = make_cylinder([0.0; 3], 1.5, 4.0).unwrap();
    let pin = make_cylinder([0.0, 0.0, 4.0], 0.15, 1.2).unwrap();
    let b = crate::boolean::boolean(base, pin, crate::BooleanOp::Union)
        .unwrap()
        .unwrap();
    shell(&b, &order(&b), &[], -0.18).unwrap();
    let want = PI * 2.25 * 4.0 + PI * 0.0225 * 1.2 - PI * 1.32_f64.powi(2) * 3.64;
    assert!(rel(volume(&b), want) < 1e-9, "{} {want}", volume(&b));
    assert!(b.is_manifold_solid().unwrap());
    assert_eq!(b.check(crate::CheckLevel::All).unwrap().failures, 0);

    let boss = make_box([14.0, 9.0, 10.0], [2.0, 2.0, 3.0]).unwrap();
    let b = crate::boolean::boolean(block(), boss, crate::BooleanOp::Union)
        .unwrap()
        .unwrap();
    shell(&b, &order(&b), &[], -1.2).unwrap();
    let want = ABC + 12.0 - 27.6 * 17.6 * 7.6;
    assert!(rel(volume(&b), want) < 1e-9, "{} {want}", volume(&b));
    assert!(b.is_manifold_solid().unwrap());
    assert_eq!(b.check(crate::CheckLevel::All).unwrap().failures, 0);
}

/// Three pins r 0.5, 2 high, on the top of the 30 x 20 x 10 block, shelled closed with a wall
/// of their radius (solidcore#3353, acid-8 stage 5's seven bosses): each pin's side collapses
/// onto its axis, so all three come off together, what is left is shelled once (two shells in
/// all, counted) and the pins are joined back on, solid.
///
/// Closed form: 6000 + 3 pi 0.5^2 2 - (30 - 1)(20 - 1)(10 - 1).
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn pins_as_thin_as_the_wall_come_off_together_and_stay_solid() {
    all_fixes();
    let mut b = block();
    for x in [8.0, 15.0, 22.0] {
        let pin = make_cylinder([x, 10.0, 10.0], 0.5, 2.0).unwrap();
        b = crate::boolean::boolean(b, pin, crate::BooleanOp::Union)
            .unwrap()
            .unwrap();
    }
    let (r, shells) = session::counted(solidcore::kernel::WorkCounters::shells, || {
        shell(&b, &order(&b), &[], -0.5)
    });
    r.unwrap();
    // the first shell, refused for the pins, and one of the block with all three off (one
    // at a time it was one more for each pin)
    assert_eq!(shells, 2);
    let want = ABC + 3.0 * PI * 0.25 * 2.0 - 29.0 * 19.0 * 9.0;
    assert!(rel(volume(&b), want) < 1e-9, "{} {want}", volume(&b));
    assert!(b.is_manifold_solid().unwrap());
    assert_eq!(b.check(crate::CheckLevel::All).unwrap().failures, 0);
}

/// The stress corpus's cut spheres and tori with a thin feature (S6226, S2226): a pin r 2,
/// 10 high, on the cut ball's cap, shelled closed with a 3 wall. The cap is the cutting box's
/// bottom, so its surface normal points into the ball, and only its other face use faces out
/// of the material. Read from the side along the normal, every edge of the cap read the
/// wrong way round: the pin's foot convex and the rim concave. So the walks from the ball and
/// from the cap both took the pin in, no feature was found, and the pin's collapse refused the
/// shell.
///
/// The hollow is the cut ball's: the ball r 47 below z = 27, V = pi (R + h)² (2R - h) / 3 for
/// R 50, h 30 less R 47, h 27, with the pin, pi 2² 10, solid on it.
#[test]
fn a_thin_pin_on_a_cut_balls_cap_stays_solid() {
    all_fixes();
    let pin = make_cylinder([0.0, 0.0, 30.0], 2.0, 10.0).unwrap();
    let b = crate::boolean::boolean(cut_ball(), pin, crate::BooleanOp::Union)
        .unwrap()
        .unwrap();
    shell(&b, &order(&b), &[], -3.0).unwrap();
    let want = PI * 6400.0 * 70.0 / 3.0 + PI * 4.0 * 10.0 - PI * 5476.0 * 67.0 / 3.0;
    assert!(rel(volume(&b), want) < 1e-9, "{} {want}", volume(&b));
    assert!(b.is_manifold_solid().unwrap());
    assert_eq!(b.check(crate::CheckLevel::All).unwrap().failures, 0);
}

/// A 1200 x 2400 cylinder (r 600) with a 140 x 140 boss (r 70) on its top whose side touches
/// the top's rim from inside (its axis 530 from the cylinder's: 530 + 70 = 600), shelled
/// closed with a 72 wall (stress corpus S6050, S0837). The boss's side offset by 72 passes its
/// radius, 70, and collapses: the shell is refused for it, naming that radius. The retry
/// with the boss taken off caps its footprint, a circle split where it touches the rim and at
/// the boss's seam. The footprint's two arcs run between the cap and the top, on one plane,
/// and at both vertices their offsets take the vertex's image as an end. Their extended UV
/// curves, each wound round the circle many times, coincide: their thousands of crossings
/// within the tolerance had some five thousand stretches certified for moving to a curve end
/// (over 90 s). The base's shell is still refused, so the first refusal stands, unchanged.
#[test]
#[allow(clippy::disallowed_methods)] // the seams' turn places the test body, not ops geometry
fn a_thin_boss_touching_its_faces_rim_is_refused_without_winding_its_footprint() {
    all_fixes();
    // each circle's seam turned 0.6131 rad about its axis, as SolidCraft's cylinders have it
    let (s, c) = 0.6131_f64.sin_cos();
    let turned = |x: f64, z: f64| {
        [
            [c, s, 0.0, 0.0],
            [-s, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [x, 0.0, z, 1.0],
        ]
    };
    let base = make_cylinder([0.0; 3], 600.0, 2400.0).unwrap();
    base.transform(&turned(0.0, 0.0)).unwrap();
    let boss = make_cylinder([0.0; 3], 70.0, 140.0).unwrap();
    boss.transform(&turned(530.0, 2400.0)).unwrap();
    let b = crate::boolean::boolean(base, boss, crate::BooleanOp::Union)
        .unwrap()
        .unwrap();
    let o = order(&b);
    let before = volume(&b);
    let (r, certified) = session::counted(
        solidcore::kernel::WorkCounters::contact_stretch_certifications,
        || shell(&b, &o, &[], -72.0),
    );
    match r {
        Err(Error::OffsetRefused {
            why: crate::OffsetRefusal::Collapses { distance, radius },
            ..
        }) => {
            assert!((distance.abs() - 72.0).abs() < 1e-9, "{distance}");
            assert!((radius - 70.0).abs() < 1e-9, "{radius}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), before) < 1e-12);
    // the shells' own intersections certify under a hundred stretches; the footprint's wound
    // extensions certified thousands
    assert!(certified < 1_000, "{certified} stretches certified");
}

/// A basin: a 100 x 60 x 30 slab less a ruled pocket (60 x 46 at the top, 4 from the front, 40 x
/// 20 at its floor 20 down), shelled open at its bottom, each wall moved in by t with sharp
/// edges. The cavity is the slab shrunk by t (open at z = 0) less the pocket grown by t, each
/// sloped side moved out along its normal; the volume, slab - pocket - cavity, is a cubic in z
/// integrated by hand. From t ~ 2.70 the front side's offset passes the front wall's below the
/// 4 mm rim, and the rim stays solid: the cavity's front stops at y = t (from
/// z = (28 - (1 + sqrt 1.64) t) / 0.8 up).
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_basin_shells_past_its_rim_to_its_closed_form() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64, z: f64| {
        let p = [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]];
        (0..4)
            .map(|i| crate::sweep::Curve::Line {
                start: p[i],
                end: p[(i + 1) % 4],
            })
            .collect::<Vec<_>>()
    };
    let pocket = crate::sweep::loft(
        &[
            rect(20.0, 4.0, 80.0, 50.0, 30.0),
            rect(30.0, 20.0, 70.0, 40.0, 10.0),
        ],
        1,
        true,
    )
    .unwrap();
    let slab = make_box([0.0; 3], [100.0, 60.0, 30.0]).unwrap();
    let basin = crate::boolean::boolean(slab, pocket, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    // The pocket grown by t at height z: x from 35 - z/2 - t sqrt5/2 to 65 + z/2 + t sqrt5/2, y
    // from 28 - 0.8 z - t sqrt1.64 (or y = t, where that is less) to 35 + z/2 + t sqrt5/2, for
    // z = 10 - t ... 30 - t; its sides stay inside the shrunk slab in x and at the back for t <= 6.
    let (r5, r164) = (5f64.sqrt(), 1.64f64.sqrt());
    let integral = |c: [f64; 3], lo: f64, hi: f64| {
        c[0] * (hi - lo) + c[1] * (hi * hi - lo * lo) / 2.0 + c[2] * (hi.powi(3) - lo.powi(3)) / 3.0
    };
    // (w0 + z)(h0 + h1 z) as coefficients in z.
    let product = |w0: f64, h0: f64, h1: f64| [w0 * h0, w0 * h1 + h0, h1];
    let grown = |t: f64| {
        let (lo, hi, w0) = (10.0 - t, 30.0 - t, 30.0 + r5 * t);
        let full = product(w0, 7.0 + t * (r5 / 2.0 + r164), 1.3);
        let z0 = ((28.0 - r164 * t - t) / 0.8).clamp(lo, hi);
        integral(full, lo, z0) + integral(product(w0, 35.0 + (r5 / 2.0 - 1.0) * t, 0.5), z0, hi)
    };
    let closed = |t: f64| {
        100.0 * 60.0 * 30.0 - grown(0.0) - (100.0 - 2.0 * t) * (60.0 - 2.0 * t) * (30.0 - t)
            + grown(t)
    };
    assert!(rel(volume(&basin), 180_000.0 - grown(0.0)) < 1e-9);
    for (t, want) in [
        (1.0, 17_105.624_041_5),
        (4.0, 63_560.249_228_4),
        (6.0, 90_113.920_210_1),
    ] {
        assert!(rel(closed(t), want) < 1e-12, "the closed form at {t}");
        let b = basin.copy().unwrap();
        let o = order(&b);
        let bottom = face(&b, &o, |bx| {
            bx.min().z.abs() < 1e-9 && bx.max().z.abs() < 1e-9
        });
        shell(&b, &o, &[bottom], -t).unwrap_or_else(|e| panic!("t {t}: {e:?}"));
        assert!(
            rel(volume(&b), want) < 1e-6,
            "t {t}: {} vs {want}",
            volume(&b)
        );
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "t {t}: {}", report.text);
    }
}

/// A frustum (r 27.5 at its foot, drafted out 1 degree, 30 high) standing on a slab 30 deep
/// whose plan is a half disc of the same radius and a 55 x 69.94 rectangle off its diameter
/// (#17469). At each end of the diameter the rectangle's side runs tangent into the half
/// disc's wall and the frustum's foot meets the ledge: four faces at one vertex. Shelled in
/// by t, open at the top and at the rectangle's far end, the side's and the ledge's offsets
/// meet at z = -t, the wall's and the frustum's at z1 = t (1 / cos a - 1) / tan a above it,
/// and the vertex's offset is the triangle between them in the plane through the axis; the
/// shell is closed. Its volume is the body less the cavity: the stadium shrunk by t from
/// z = t - 30 up to -t, then the half disc of radius 27.5 - t and the frustum's offset half
/// (radius 27.5 + z tan a - t / cos a) up to z1, then the frustum's offset up to the top.
#[test]
fn a_frustum_on_a_ledge_tangent_into_its_wall_shells_closed() {
    all_fixes();
    // ops computes no trigonometry, so the draft's numbers are written out.
    // tan(1 degree).
    let tan = 0.017_455_064_928_217_585;
    // cos(1 degree).
    let cos = 0.999_847_695_156_391_3;
    let (r, h, len) = (27.5, 30.0, 69.94);
    let body = crate::boolean::union_all(vec![
        crate::make_cone([0.0; 3], r, r + h * tan, h).unwrap(),
        make_cylinder([0.0, 0.0, -h], r, h).unwrap(),
        make_box([-r, 0.0, -h], [2.0 * r, len, h]).unwrap(),
    ])
    .unwrap();
    // the frustum's top radius
    let top = r + h * tan;
    let frustum = PI * h / 3.0 * (r * r + r * top + top * top);
    let whole = frustum + (PI * r * r / 2.0 + 2.0 * r * len) * h;
    assert!(rel(volume(&body), whole) < 1e-9);
    for t in [0.5, 1.0, 2.0] {
        let wall = r - t;
        let inner = |z: f64| r + z * tan - t / cos;
        // the integral of inner(z)^2 from z0 to z1
        let cube = |z: f64| inner(z) * inner(z) * inner(z);
        let squared = |z0: f64, z1: f64| (cube(z1) - cube(z0)) / (3.0 * tan);
        let z1 = t * (1.0 / cos - 1.0) / tan;
        let cavity = (PI * wall * wall / 2.0 + 2.0 * wall * len) * (h - 2.0 * t)
            + PI * wall * wall / 2.0 * (z1 + t)
            + PI / 2.0 * squared(-t, z1)
            + PI * squared(z1, h);
        let b = body.copy().unwrap();
        let o = order(&b);
        let end = face(&b, &o, |bx| {
            (bx.min().y - len).abs() < 1e-9 && (bx.max().y - len).abs() < 1e-9
        });
        let lid = face(&b, &o, |bx| {
            (bx.min().z - h).abs() < 1e-9 && (bx.max().z - h).abs() < 1e-9
        });
        shell(&b, &o, &[end, lid], -t).unwrap_or_else(|e| panic!("t {t}: {e:?}"));
        assert!(
            rel(volume(&b), whole - cavity) < 1e-9,
            "t {t}: {} vs {}",
            volume(&b),
            whole - cavity
        );
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "t {t}: {}", report.text);
    }
}

#[test]
fn a_wall_with_no_room_is_refused_before_the_kernel() {
    all_fixes();
    // a wall of 15 inside a block 20 wide leaves no cavity
    let b = block();
    let o = order(&b);
    assert_eq!(shell(&b, &o, &[top(&b, &o)], -15.0), Err(Error::NoRoom));
    // the top moved in 10, as deep as the block: nothing behind it
    let b = block();
    let o = order(&b);
    assert_eq!(
        offset_faces(&b, &o, &[top(&b, &o)], -10.0),
        Err(Error::NoRoom)
    );
}

#[test]
fn the_top_moves_out() {
    all_fixes();
    let b = block();
    let o = order(&b);
    offset_faces(&b, &o, &[top(&b, &o)], 2.0).unwrap();
    assert!(rel(volume(&b), ABC + 2.0 * 600.0) < 1e-9);
}

/// `body` with its face through `at` split by the plane `z = height`.
fn split_at_height(body: &Body, at: [f64; 3], height: f64) -> Body {
    let plane = ([0.0, 0.0, height], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    split_by_plane(body, at, plane)
}

/// `body` with its face through `at` split by the plane through `origin` along `u` and `v`.
fn split_by_plane(
    body: &Body,
    at: [f64; 3],
    (origin, u, v): ([f64; 3], [f64; 3], [f64; 3]),
) -> Body {
    let o = order(body);
    let tool = crate::split::Tool {
        surface: solidcore_record::brep::Surface::Plane {
            origin,
            u_axis: u,
            v_axis: v,
        },
        domain: [[-200.0, 200.0], [-200.0, 200.0]],
    };
    let (split, _) = crate::split::split_faces(body, &o, &[through(body, &o, at)], &[tool])
        .unwrap()
        .unwrap();
    split
}

/// The record index (through `o`) of the one face of `b` through `p`.
fn through(b: &Body, o: &Order, p: [f64; 3]) -> usize {
    let hits: Vec<usize> = (0..o.faces.len())
        .filter(|&i| b.face_distances(o, i, &[p]).unwrap()[0].is_some_and(|d| d < 1e-9))
        .collect();
    assert_eq!(hits.len(), 1, "faces through {p:?}: {hits:?}");
    hits[0]
}

/// The faces of `b` through `points` (each once) moved `d`: one closed, sound solid of `expected` volume,
/// with `counts` faces, edges and vertices.
fn moved_to(b: &Body, points: &[[f64; 3]], d: f64, expected: f64, counts: [usize; 3]) {
    let o = order(b);
    let mut faces: Vec<usize> = points.iter().map(|&p| through(b, &o, p)).collect();
    faces.sort_unstable();
    faces.dedup();
    offset_faces(b, &o, &faces, d).unwrap();
    let measured = volume(b);
    assert!(rel(measured, expected) < 1e-9, "{measured} vs {expected}");
    assert_eq!(b.counts().unwrap(), counts);
    assert!(b.is_manifold_solid().unwrap());
    b.check_solid().unwrap();
    let report = b.check(crate::CheckLevel::All).unwrap();
    assert_eq!(report.failures, 0, "{}", report.text);
}

/// The upper half of a cylinder's side (r 50, z 0 to 100, split at z = 50) moved out 4:
/// the cylinder r 54 beside the cylinder r 50 never meets it, and the annulus at z = 50
/// closes the step. V = 2500 pi 100 + (54² - 50²) pi 50 = 850743.291. The step is one face
/// with a seam of its own beside the sides' seams: 5 faces, 7 edges (a circle at z = 0 and
/// at z = 100, two at z = 50, three seams), 4 vertices.
#[test]
fn half_a_cylinders_side_moves_out_over_a_step() {
    all_fixes();
    let b = split_at_height(
        &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
        [50.0, 0.0, 25.0],
        50.0,
    );
    let v = PI * 2500.0 * 100.0 + PI * (54.0 * 54.0 - 2500.0) * 50.0;
    moved_to(&b, &[[50.0, 0.0, 75.0]], 4.0, v, [5, 7, 4]);
}

/// [`half_a_cylinders_side_moves_out_over_a_step`] with only the upper band split along its
/// rulings by the plane x = 0 and the whole lower band moved out 4: where such a ruling
/// meets the circle at z = 50, the two kept upper pieces are one surface sharing a straight
/// edge, and the third surface at the vertex is the move's own cylinder (a quadric, not a
/// plane). Every two of the three coaxial cylinders are parallel, so they meet in no one
/// point and the annulus at z = 50 closes the step, as the hinge's knuckle (a barrel piece
/// beside the coaxial piece it was split from, plus a straight kept edge) needs.
/// V = 2500 pi 50 + 2916 pi 50 = 850743.291, with the split's 9 faces, 17 edges and 10
/// vertices (the same split body as `three_cylinder_faces_at_a_corner_move_out_over_a_step`,
/// the whole lower band moved instead of the three upper pieces).
#[test]
fn a_cylinders_whole_band_moves_out_beside_its_split_kept_halves() {
    all_fixes();
    let halves = split_at_height(
        &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
        [50.0, 0.0, 25.0],
        50.0,
    );
    let b = split_by_plane(
        &halves,
        [-50.0, 0.0, 75.0],
        ([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    );
    let v = PI * 2500.0 * 50.0 + PI * 2916.0 * 50.0;
    moved_to(&b, &[[50.0, 0.0, 25.0]], 4.0, v, [9, 17, 10]);
}

/// Both halves of that side moved out 4 together: one cylinder r 54, V = 2916 pi 100, with
/// the split's 4 faces, 5 edges and 3 vertices.
#[test]
fn both_halves_of_a_cylinders_side_move_out_as_one() {
    all_fixes();
    let b = split_at_height(
        &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
        [50.0, 0.0, 25.0],
        50.0,
    );
    let both = [[50.0, 0.0, 25.0], [50.0, 0.0, 75.0]];
    moved_to(&b, &both, 4.0, PI * 2916.0 * 100.0, [4, 5, 3]);
}

/// A sphere r 50's cap above z = 20 moved out 4: the sphere r 54 beside the sphere r 50
/// never meets it, and the cone of normals through the cap's rim closes the step. The
/// added shell sector is (2 pi / 3)(54³ - 50³)(1 - 20 / 50), so
/// V = (4 pi / 3) 50³ + (2 pi / 3)(54³ - 50³) 0.6 = 564394.241. The split leaves the rim as
/// two edges, and each closes with a face of that cone: 4 faces, 8 edges (the rim's two on
/// each sphere, two lines between the cone's faces, a seam on each sphere), 6 vertices.
#[test]
fn a_spheres_cap_moves_out_over_a_step() {
    all_fixes();
    let b = split_at_height(
        &make_sphere([0.0; 3], 50.0).unwrap(),
        [0.0, 0.0, 50.0],
        20.0,
    );
    let v = 4.0 * PI / 3.0 * 125_000.0 + 2.0 * PI / 3.0 * (157_464.0 - 125_000.0) * 0.6;
    moved_to(&b, &[[0.0, 0.0, 50.0]], 4.0, v, [4, 8, 6]);
}

/// A sphere r 50 with its top cut off at z = 30: the flat cap, a disc of r 40, meets the
/// sphere at an angle along its rim, and the sphere's top is 20 above it.
fn cut_ball() -> Body {
    let ball = make_sphere([0.0; 3], 50.0).unwrap();
    let above = make_box([-60.0, -60.0, 30.0], [120.0, 120.0, 40.0]).unwrap();
    crate::boolean::boolean(ball, above, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// The cut ball's cap moved out 10, to z = 40, still meets the sphere: the ball below
/// z = 40, V = pi (R + h)² (2R - h) / 3 = pi 90² 60 / 3.
#[test]
fn a_cut_balls_cap_moves_out_within_the_sphere() {
    all_fixes();
    let b = cut_ball();
    let o = order(&b);
    offset_faces(&b, &o, &[through(&b, &o, [0.0, 0.0, 30.0])], 10.0).unwrap();
    let expected = PI * 8100.0 * 60.0 / 3.0;
    assert!(rel(volume(&b), expected) < 1e-9, "{}", volume(&b));
}

/// The cut ball's cap moved out 25, to z = 55, past the sphere's top at z = 50: the sphere
/// beside it can't be extended to meet it, so the move is refused before anything changes,
/// naming the cap and how far it could move, 20 (#18564). It was refused late, as a body
/// that is not a manifold solid (or, hollowed, as a move that moved no face).
#[test]
fn a_cut_balls_cap_moved_past_the_sphere_is_refused_with_the_room() {
    all_fixes();
    let b = cut_ball();
    let o = order(&b);
    let cap = through(&b, &o, [0.0, 0.0, 30.0]);
    let before = volume(&b);
    match offset_faces(&b, &o, &[cap], 25.0) {
        Err(Error::OffsetRefused {
            face,
            why: crate::OffsetRefusal::PastSide { largest },
        }) => {
            assert_eq!(face, Some(cap));
            assert!((largest - 20.0).abs() < 1e-9, "{largest}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), before) < 1e-12);
}

/// The upper half of a frustum's side (r 50 at z = 0 to r 30 at z = 100, split at z = 50)
/// moved out 4: the cone 4 out beside the cone it came from never meets it, and the cone of
/// normals through the split circle closes the step. In a half-plane through the axis the
/// added region is A = (40, 50), B = A + 4 n, C = (30 + 4 √1.04, 100), D = (30, 100),
/// n = (1, 0.2) / √1.04 the side's normal, and V = 4900 pi 100 / 3 + 2 pi ∫∫ r dr dz over
/// it. Its faces, edges and vertices are the cylinder's.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn half_a_cones_side_moves_out_over_a_step() {
    all_fixes();
    let b = split_at_height(
        &crate::make_cone([0.0; 3], 50.0, 30.0, 100.0).unwrap(),
        [45.0, 0.0, 25.0],
        50.0,
    );
    let w = 1.04_f64.sqrt();
    let region = [
        (40.0, 50.0),
        (40.0 + 4.0 / w, 50.0 + 0.8 / w),
        (30.0 + 4.0 * w, 100.0),
        (30.0, 100.0),
    ];
    // ∫∫ r dr dz over the polygon, by Green's theorem
    let moment: f64 = (0..4)
        .map(|i| {
            let ((r0, z0), (r1, z1)) = (region[i], region[(i + 1) % 4]);
            (r0 + r1) * (r0 * z1 - r1 * z0) / 6.0
        })
        .sum();
    let v = PI * 4900.0 * 100.0 / 3.0 + 2.0 * PI * moment.abs();
    moved_to(&b, &[[35.0, 0.0, 75.0]], 4.0, v, [5, 7, 4]);
}

/// A cylinder's side (r 50, z 0 to 100) split along its rulings by the plane x = 0, with the
/// half at x > 0 moved out 4. At each end of a ruling a cap, the cylinder r 54 and the
/// cylinder r 50 meet in no one point; the caps run on to the moved half, and a strip of the
/// plane x = 0 closes each step. V = 2500 pi 100 + (pi / 2)(54² - 50²) 100. The side's seam
/// at x = 50 splits the moved half in two: 7 faces (two caps, the kept half, two moved
/// quarters, two strips), 15 edges (five on each cap, five up the side), 10 vertices.
#[test]
fn half_a_cylinders_side_split_along_its_rulings_moves_out_over_steps() {
    all_fixes();
    let b = split_by_plane(
        &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
        [0.0, 50.0, 50.0],
        ([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    );
    let v = PI * 2500.0 * 100.0 + PI / 2.0 * (54.0 * 54.0 - 2500.0) * 100.0;
    // on either side of the side's seam, at x = 50
    let half = [[30.0, 40.0, 50.0], [30.0, -40.0, 50.0]];
    moved_to(&b, &half, 4.0, v, [7, 15, 10]);
}

/// The upper half of that cylinder's side (split at z = 50) moved in 4: the cylinder r 46
/// inside the cylinder r 50, the annulus at z = 50 facing up.
/// V = 2500 pi 100 - (50² - 46²) pi 50, with the outward move's 5 faces, 7 edges, 4 vertices.
#[test]
fn half_a_cylinders_side_moves_in_under_a_step() {
    all_fixes();
    let b = split_at_height(
        &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
        [50.0, 0.0, 25.0],
        50.0,
    );
    let v = PI * 2500.0 * 100.0 - PI * (2500.0 - 46.0 * 46.0) * 50.0;
    moved_to(&b, &[[50.0, 0.0, 75.0]], -4.0, v, [5, 7, 4]);
}

/// A cylinder's side split along its rulings (a round stored as two halves, the half at
/// x > 0 in two by the seam) moved out 4 whole: one cylinder r 54, V = 2916 pi 100, with
/// the split's 5 faces, 9 edges (three arcs on each cap, three rulings), 6 vertices.
#[test]
fn a_cylinders_side_split_along_its_rulings_moves_out_as_one() {
    all_fixes();
    let b = split_by_plane(
        &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
        [0.0, 50.0, 50.0],
        ([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    );
    let all = [[-50.0, 0.0, 50.0], [30.0, 40.0, 50.0], [30.0, -40.0, 50.0]];
    moved_to(&b, &all, 4.0, PI * 2916.0 * 100.0, [5, 9, 6]);
}

/// Pieces of split cylinders and cones given surfaces a few roundings off the kept piece's
/// axis or slope, or a real angle off it (#13722), and three cylinder faces at a corner.
mod near_parallel {
    use super::*;
    use crate::session::with_body;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::offset::draft::Draft;
    use solidcore::offset::offset_executive::p2::local_operation_simple;
    use solidcore::surface::implicit::AnalyticSupport;
    use solidcore::surface::surface::{Surface, SurfaceKind};
    use solidcore::{FaceId, Vec3};

    /// The faces of `b` through `points` (each once) given, by the kernel's local operation, the
    /// surface `given` makes for each: the body after it, or the kernel's refusal (the body then
    /// as it came in).
    fn given_surfaces(
        b: &Body,
        points: &[[f64; 3]],
        given: &dyn Fn(&mut Kernel, &solidcore::Brep, FaceId) -> Surface,
    ) -> solidcore::Result<()> {
        let o = order(b);
        let mut faces: Vec<FaceId> = points.iter().map(|&p| o.faces[through(b, &o, p)]).collect();
        faces.sort_unstable();
        faces.dedup();
        let mut refusal = None;
        with_body("LocalOperation", b, |k, brep| {
            let surfaces = faces.iter().map(|&f| given(k, brep, f)).collect();
            if let Err(e) = local_operation_simple(k, brep, &faces, surfaces) {
                refusal = Some(e);
            }
            Ok(refusal.is_none())
        })
        .unwrap();
        refusal.map_or(Ok(()), Err)
    }

    /// `b` one closed, sound solid of `expected` volume, with `counts` faces, edges and vertices.
    fn sound(b: &Body, expected: f64, counts: [usize; 3]) {
        let measured = volume(b);
        assert!(rel(measured, expected) < 1e-9, "{measured} vs {expected}");
        assert_eq!(b.counts().unwrap(), counts);
        assert!(b.is_manifold_solid().unwrap());
        b.check_solid().unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "{}", report.text);
    }

    /// A cylinder's or cone's axis, the radius at its origin and its semi-angle in degrees.
    #[allow(clippy::disallowed_methods)] // reads the premise back, not ops geometry
    fn support(s: &Surface) -> (Vec3, f64, f64) {
        let axis = |p: &Axis2Placement| p.x_axis().cross(p.y_axis());
        match AnalyticSupport::of(s) {
            Some(AnalyticSupport::Cylinder { placement, radius }) => {
                (axis(&placement), radius, 0.0)
            }
            Some(AnalyticSupport::Cone {
                placement,
                base_radius,
                semi_angle_deg,
            }) => (axis(&placement), base_radius, semi_angle_deg),
            other => panic!("not a cylinder or a cone: {other:?}"),
        }
    }

    /// The surface of face `f` of `b`, a cylinder or cone, moved out 4 along its normals (its
    /// offset, the parameters kept), then turned about the axis along (1, 2, 0) through (0, 0, z) by
    /// each (angle, z) of `turns` (radians) and drafted about its circle at z = 50 by each of `drafts` (radians),
    /// its NURBS form with it: turned and drafted there and back, an offset computed apart from
    /// the surface it moves off, its axis or slope a few roundings off.
    #[allow(clippy::disallowed_methods)] // the test input's turn, not ops geometry
    fn moved_out(
        k: &Kernel,
        b: &solidcore::Brep,
        f: FaceId,
        turns: &[(f64, f64)],
        drafts: &[f64],
    ) -> Surface {
        let own = b.face(f).unwrap().surface.as_ref().unwrap();
        let (axis, radius, semi) = support(own);
        let offset = |d: f64| own.create_offset_surface(k, d, 1e-9).unwrap().unwrap();
        let mut moved = offset(4.0);
        // out: the larger radius at the origin
        if support(&moved).1 < radius {
            moved = offset(-4.0);
        }
        for &(turn, z) in turns {
            let mut turned = Axis2Placement::IDENTITY;
            turned.rotate_about_axis_at_point(
                turn,
                Vec3::new(0.0, 0.0, z),
                Vec3::new(1.0, 2.0, 0.0),
            );
            moved.transform(k, &turned, None).unwrap();
        }
        for &angle in drafts {
            let draft = Draft::new(Vec3::new(0.0, 0.0, 50.0), Vec3::Z, Vec3::Z, angle).unwrap();
            let SurfaceKind::Cone(cone) = moved.kind() else {
                panic!("not a cone");
            };
            moved = Surface::from(cone.drafted_about_plane(k, true, &draft).unwrap().unwrap());
        }
        // the premise: the axis or the slope is not the kept surface's
        let (moved_axis, _, moved_semi) = support(&moved);
        assert!(
            moved_axis != axis || moved_semi.to_bits() != semi.to_bits(),
            "{moved_axis:?} {moved_semi}"
        );
        moved
    }

    /// SolidCraft's `cyl_half_band` (#13722): the upper half of a cylinder's side (r 50, z 0 to
    /// 100, split at z = 50) given its offset r 54 turned 3e-16 radians about an axis along
    /// (1, 2, 0) through (0, 0, 1e5), so its axis is a few roundings off its own (as SolidCraft's
    /// offset came out about `(2.3245e-16, 1.6354e-16, 1)`) and about 3e-11 off its line. The
    /// axes are not one vector, so the exact parallel test does not take them, but over the body
    /// the two cylinders stay within 1e-10 of coaxial, 4 apart: the annulus at z = 50 closes the
    /// step, as for the exact axis. V = 2500 pi 100 + (54² - 50²) pi 50 = 850743.291 (the turn
    /// moves the moved side by under 1e-10 over its area of 2 pi 54 50, so the volume by under
    /// 2e-6, a relative 3e-12), with the exact axis's 5 faces, 7 edges and 4 vertices.
    #[test]
    fn half_a_cylinders_side_moves_out_on_an_axis_a_few_roundings_off() {
        all_fixes();
        let b = split_at_height(
            &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
            [50.0, 0.0, 25.0],
            50.0,
        );
        given_surfaces(&b, &[[50.0, 0.0, 75.0]], &|k, brep, f| {
            moved_out(k, brep, f, &[(3e-16, 1e5)], &[])
        })
        .unwrap();
        let v = PI * 2500.0 * 100.0 + PI * (54.0 * 54.0 - 2500.0) * 50.0;
        sound(&b, v, [5, 7, 4]);
    }

    /// The volume of the frustum r 50 at z = 0 to r 30 at z = 100 with the upper half of its side
    /// moved out 4 ([`half_a_cones_side_moves_out_over_a_step`]).
    #[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
    fn half_cone_moved_out_volume() -> f64 {
        let w = 1.04_f64.sqrt();
        let region = [
            (40.0, 50.0),
            (40.0 + 4.0 / w, 50.0 + 0.8 / w),
            (30.0 + 4.0 * w, 100.0),
            (30.0, 100.0),
        ];
        // ∫∫ r dr dz over the polygon, by Green's theorem
        let moment: f64 = (0..4)
            .map(|i| {
                let ((r0, z0), (r1, z1)) = (region[i], region[(i + 1) % 4]);
                (r0 + r1) * (r0 * z1 - r1 * z0) / 6.0
            })
            .sum();
        PI * 4900.0 * 100.0 / 3.0 + 2.0 * PI * moment.abs()
    }

    /// A cone isogrid's band (#13722): the upper half of a frustum's side (r 50 at z = 0 to r 30
    /// at z = 100, split at z = 50) given its offset 4 out drafted 0.01 radians about its circle
    /// at z = 50 and back, so its slope is a few roundings off the kept half's (as SolidCraft's
    /// offset came out,
    /// `tan_angle` −0.22499999999999998 against −0.22499999999999995). The slopes are not one
    /// number, so the exact parallel test does not take them, but
    /// over the body the cones stay within 1e-13 of parallel, 4 apart: the cone of normals
    /// through the split circle closes the step, as for the exact slope. V as
    /// [`half_a_cones_side_moves_out_over_a_step`] (the slope moves the moved side by at most
    /// 1e-16 x 100 over its area, so the volume by a relative 1e-15), with its faces, edges and
    /// vertices.
    #[test]
    fn half_a_cones_side_moves_out_on_a_slope_a_few_roundings_off() {
        all_fixes();
        let b = split_at_height(
            &crate::make_cone([0.0; 3], 50.0, 30.0, 100.0).unwrap(),
            [45.0, 0.0, 25.0],
            50.0,
        );
        given_surfaces(&b, &[[35.0, 0.0, 75.0]], &|k, brep, f| {
            moved_out(k, brep, f, &[], &[0.01, -0.01])
        })
        .unwrap();
        sound(&b, half_cone_moved_out_volume(), [5, 7, 4]);
    }

    /// A cylinder's side (r 50, z 0 to 100) split at z = 50 and its upper half split along its
    /// rulings by the plane x = 0, the upper pieces moved out 4: where a ruling meets the circle at
    /// z = 50, three cylinder faces meet at a vertex with no plane, two moved to the cylinder
    /// r 54 and the kept one r 50, every two of them one surface or apart: they meet in no one
    /// point, and the annulus at z = 50 closes the step, as for the upper half moved whole.
    /// V = 2500 pi 100 + (54² - 50²) pi 50 = 850743.291.
    #[test]
    fn three_cylinder_faces_at_a_corner_move_out_over_a_step() {
        all_fixes();
        let halves = split_at_height(
            &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
            [50.0, 0.0, 25.0],
            50.0,
        );
        let b = split_by_plane(
            &halves,
            [-50.0, 0.0, 75.0],
            ([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        );
        let upper = [[-50.0, 0.0, 75.0], [30.0, 40.0, 75.0], [30.0, -40.0, 75.0]];
        let v = PI * 2500.0 * 100.0 + PI * (54.0 * 54.0 - 2500.0) * 50.0;
        moved_to(&b, &upper, 4.0, v, [9, 17, 10]);
    }

    /// [`three_cylinder_faces_at_a_corner_move_out_over_a_step`] with each upper piece given its
    /// offset r 54 turned 3e-16 radians about the axis along (1, 2, 0) through (0, 0, 1e5), as
    /// [`half_a_cylinders_side_moves_out_on_an_axis_a_few_roundings_off`] turns the band: at the
    /// corners where a ruling meets the circle at z = 50, the kept lower band and the two moved
    /// pieces are three cylinders every two of which are parallel, a pair of them only within
    /// a ball (the turned axis is not the kept one's vector), so the corner's decision holds only
    /// where the face's extension lies inside that ball, which is checked. The cylinders stay
    /// within 1e-10 of coaxial over the body: V = 2500 pi 100 + (54² - 50²) pi 50 = 850743.291
    /// (the turn moves the moved sides by under 1e-10 over their area, a relative 3e-12), with
    /// the exact move's 9 faces, 17 edges and 10 vertices.
    #[test]
    fn three_cylinder_faces_at_a_corner_move_out_on_an_axis_a_few_roundings_off() {
        all_fixes();
        let halves = split_at_height(
            &make_cylinder([0.0; 3], 50.0, 100.0).unwrap(),
            [50.0, 0.0, 25.0],
            50.0,
        );
        let b = split_by_plane(
            &halves,
            [-50.0, 0.0, 75.0],
            ([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        );
        let upper = [[-50.0, 0.0, 75.0], [30.0, 40.0, 75.0], [30.0, -40.0, 75.0]];
        given_surfaces(&b, &upper, &|k, brep, f| {
            moved_out(k, brep, f, &[(3e-16, 1e5)], &[])
        })
        .unwrap();
        let v = PI * 2500.0 * 100.0 + PI * (54.0 * 54.0 - 2500.0) * 50.0;
        sound(&b, v, [9, 17, 10]);
    }
}

#[test]
fn hostile_arguments_are_refused() {
    all_fixes();
    let b = block();
    let o = order(&b);
    for t in [0.0, f64::NAN, 2e6] {
        assert_eq!(shell(&b, &o, &[], t), Err(Error::Argument));
        assert_eq!(offset_faces(&b, &o, &[0], t), Err(Error::Argument));
        assert_eq!(thicken(&b, t, false), Err(Error::Argument));
        assert_eq!(offset_sheet(&b, t), Err(Error::Argument));
    }
    assert_eq!(
        shell(&b, &o, &[6], -1.0),
        Err(Error::Argument),
        "an index past the faces"
    );
    assert_eq!(
        shell(&b, &o, &[1, 1], -1.0),
        Err(Error::Argument),
        "an index twice"
    );
    assert_eq!(
        offset_faces(&b, &o, &[], 1.0),
        Err(Error::Argument),
        "no faces"
    );
    assert_eq!(
        thicken(&b, 2.0, false),
        Err(Error::Argument),
        "a solid is not thickened"
    );
    assert_eq!(
        volume(&b),
        volume(&block()),
        "refused before the kernel, the body is as it was"
    );
}

/// The 30 x 20 plate in z = 0 (the x axis swept along y).
fn plate() -> Body {
    run("plate", |k| {
        let line = solidcore::curve::line::Line::create_line_segment(
            3,
            solidcore::Vec3::ZERO,
            solidcore::Vec3::new(30.0, 0.0, 0.0),
            None,
        )?;
        let c = solidcore::curve::curve::Curve::from(line);
        solidcore::sweep::api_brep::create_linear_sweep(
            k,
            &[Some(&c)],
            solidcore::Vec3::new(0.0, 1.0, 0.0),
            20.0,
            false,
        )
    })
    .map(Body::new)
    .unwrap()
}

#[test]
fn a_plate_thickens_and_offsets() {
    all_fixes();
    let p = plate();
    thicken(&p, 2.0, false).unwrap();
    assert!(rel(volume(&p), 1200.0) < 1e-9);
    let p = plate();
    thicken(&p, 2.0, true).unwrap();
    assert!(rel(volume(&p), 1200.0) < 1e-9);
    assert_eq!(p.counts().unwrap()[0], 6);
}

/// The `l` x `h` plate in y = 0 (x across, z up) with its two corners on x = 0 rounded at
/// `r`: four lines and two quarter circles, each tangent to the lines beside it.
fn plate_with_round_corners(l: f64, h: f64, r: f64) -> Body {
    use crate::sweep::Curve;
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], 0.0, a[1]],
        end: [b[0], 0.0, b[1]],
    };
    // a quarter turn about +y from the direction `from` off the centre `c`
    let corner = |c: [f64; 2], from: [f64; 3]| Curve::Arc {
        center: [c[0], 0.0, c[1]],
        axis: [0.0, 1.0, 0.0],
        reference: from,
        radius: r,
        start: 0.0,
        end: PI / 2.0,
    };
    crate::sweep::planar_faces(&[
        line([l, 0.0], [r, 0.0]),
        corner([r, r], [0.0, 0.0, -1.0]),
        line([0.0, r], [0.0, h - r]),
        corner([r, h - r], [-1.0, 0.0, 0.0]),
        line([r, h], [l, h]),
        line([l, h], [l, 0.0]),
    ])
    .unwrap()
}

/// A flat sheet with two rounded corners thickens to the prism of its outline: its area,
/// `l h - 2 (1 - pi/4) r^2`, times the thickness, in eight faces (the sheet's two copies
/// and a wall for each of its six edges), eighteen edges and twelve vertices, from the
/// sheet's plane to the thickness off it, or half the thickness to either side.
///
/// Each quarter circle's offset is trimmed on its own curve continued ten lengths each
/// way, which runs round the circle five times, so each of its ends has a parameter on
/// every turn. An end taken on another turn gave the corner the other three quarters of
/// its circle, and the thicken failed (status 1001): which turn was taken changed with
/// the radius, the thickness and the side.
#[test]
fn a_plate_with_round_corners_thickens_to_its_prism() {
    all_fixes();
    let (l, h) = (30.0, 40.0);
    for r in [16.0, 17.0, 18.0] {
        let area = l * h - 2.0 * (1.0 - PI / 4.0) * r * r;
        for t in [2.0, 104.0] {
            for symmetric in [false, true] {
                let p = plate_with_round_corners(l, h, r);
                thicken(&p, t, symmetric)
                    .unwrap_or_else(|e| panic!("r {r}, by {t}, symmetric {symmetric}: {e:?}"));
                let got = volume(&p);
                assert!(
                    rel(got, area * t) < 1e-9,
                    "r {r}, by {t}, symmetric {symmetric}: {got} against {}",
                    area * t
                );
                assert_eq!(
                    p.counts().unwrap(),
                    [8, 18, 12],
                    "r {r}, by {t}, symmetric {symmetric}"
                );
                let (low, high) = p.bounds().unwrap();
                let reach = if symmetric { 0.5 * t } else { t };
                assert!(
                    (high[1] - low[1] - t).abs() < 1e-9
                        && (low[1].abs().max(high[1].abs()) - reach).abs() < 1e-9,
                    "r {r}, by {t}, symmetric {symmetric}: {low:?} {high:?}"
                );
            }
        }
    }
}

#[test]
fn a_ball_offsets_as_a_sheet() {
    all_fixes();
    // r 10 moved out 2: the ball of radius 12
    let s = make_sphere([0.0; 3], 10.0).unwrap();
    offset_sheet(&s, 2.0).unwrap();
    assert!(rel(volume(&s), 4.0 / 3.0 * PI * 1728.0) < 1e-6);
    // moved in 10, to its centre: nothing is left
    let s = make_sphere([0.0; 3], 10.0).unwrap();
    assert_eq!(offset_sheet(&s, -10.0), Err(Error::Empty));
}

/// A Y of three walls meeting along one edge (a sheet, not manifold there): thickened, its
/// result is no closed solid. Refused as `NotSolidBecause(NotManifold)` (with the input's
/// contract it passed, the input being a sheet), the sheet as it came in.
#[test]
fn a_thickened_sheet_that_is_no_solid_is_refused() {
    all_fixes();
    let o = solidcore::Vec3::ZERO;
    let arms = [
        solidcore::Vec3::new(10.0, 0.0, 0.0),
        solidcore::Vec3::new(-5.0, 8.0, 0.0),
        solidcore::Vec3::new(-5.0, -8.0, 0.0),
    ];
    let y = run("y", |k| {
        let cs: Vec<solidcore::curve::curve::Curve> = arms
            .iter()
            .map(|&a| {
                solidcore::curve::line::Line::create_line_segment(3, o, a, None)
                    .map(solidcore::curve::curve::Curve::from)
            })
            .collect::<solidcore::Result<_>>()?;
        let refs: Vec<Option<&solidcore::curve::curve::Curve>> = cs.iter().map(Some).collect();
        solidcore::sweep::api_brep::create_linear_sweep(
            k,
            &refs,
            solidcore::Vec3::new(0.0, 0.0, 1.0),
            10.0,
            false,
        )
    })
    .map(Body::new)
    .unwrap();
    let before = y.to_smb(false).unwrap();
    assert_eq!(
        thicken(&y, 1.0, false),
        Err(Error::NotSolidBecause(crate::SolidTest::NotManifold))
    );
    assert_eq!(y.to_smb(false).unwrap(), before);
}

/// The offset statuses as the operations' errors: walls that touch, a wall that folds (with
/// the largest that works), a vertex moved past an edge's other end; others as the kernel's.
#[test]
fn the_offset_refusals_are_typed() {
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::Failure;
    use solidcore::error::{SM_ERR_OFFSET_EDGE_VANISHES, SM_ERR_OFFSET_WALLS_TOUCH};
    use solidcore::{FaceId, SolidError};
    let none = |_: solidcore::EntityRef| -> Option<crate::RecordEntity> { None };
    let up = |code| SolidError::Upstream {
        operation: "test",
        code,
        entity: None,
    };
    assert_eq!(
        super::status(up(SM_ERR_OFFSET_WALLS_TOUCH), &none),
        Error::WallsTouch
    );
    assert_eq!(
        super::status(up(SM_ERR_OFFSET_EDGE_VANISHES), &none),
        Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::EdgeVanishes,
        }
    );
    // the kernel's typed offset refusals (solidcore#2615)
    let refused = |o| Failure::new("test", o).into_error();
    assert_eq!(
        super::status(
            refused(OffsetFailure::Folds {
                face: FaceId::from_index(3),
                distance: -11.0,
                largest: 10.0,
            }),
            &none,
        ),
        Error::WallFolds {
            largest: Some(10.0)
        }
    );
    assert_eq!(
        super::status(
            refused(OffsetFailure::WallsTouch {
                face: None,
                contact: solidcore::error::offset::WallContact::Touch,
            }),
            &none,
        ),
        Error::WallsTouch
    );
    assert_eq!(
        super::status(
            refused(OffsetFailure::EdgeVanishes {
                edge: None,
                face: None,
            }),
            &none,
        ),
        Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::EdgeVanishes,
        }
    );
    // an unresolved crossing says how it crosses (#14783), no longer a bare status
    assert_eq!(
        super::status(
            refused(OffsetFailure::LoopsCross {
                face: None,
                crossing: solidcore::error::offset::LoopCrossing::AtLoopVertex,
            }),
            &none,
        ),
        Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::LoopsCross {
                crossing: solidcore::error::offset::LoopCrossing::AtLoopVertex,
            },
        }
    );
    assert_eq!(super::status(up(1001), &none), Error::Status(1001));
}

/// Each of the kernel's other offset refusals as [`Error::OffsetRefused`] (#14783): its reason,
/// and its face by the index the call's lookup gives, `None` for a face it does not find or a
/// refusal that names none. Never the bare status they were before, and never the kernel's text
/// in place of a reason (#15035): a routine's own failure keeps its kind and the kernel's phrase.
#[test]
fn every_other_offset_refusal_says_why_and_where() {
    use crate::{OffsetRefusal as R, RecordEntity, Unsoundness};
    use solidcore::error::offset::{LoopCrossing, OffsetFailure as O, Unsoundness as K};
    use solidcore::error::ErrorKind;
    use solidcore::error::Failure;
    use solidcore::FaceId;
    let named = FaceId::from_index(3);
    let other = FaceId::from_index(4);
    // the call's lookup: face 3 is record face 7, face 4 is not the body's
    let index = |e: solidcore::EntityRef| {
        (e == solidcore::EntityRef::Face(named)).then_some(RecordEntity::Face(7))
    };
    let refused = |o: O| super::status(Failure::new("test", o).into_error(), &index);
    let cases = [
        // (the kernel's fold kind, but no fold: its own reason, not `WallFolds`)
        (
            O::NotCertified {
                face: named,
                distance: -2.0,
            },
            Some(7),
            R::NotCertified { distance: -2.0 },
        ),
        (
            O::Collapses {
                face: named,
                distance: -5.0,
                radius: 5.0,
            },
            Some(7),
            R::Collapses {
                distance: -5.0,
                radius: 5.0,
            },
        ),
        (
            O::WallsMeetAcross {
                face: named,
                distance: 1.0,
                largest: 1.0,
            },
            Some(7),
            R::WallsMeetAcross {
                distance: 1.0,
                largest: 1.0,
            },
        ),
        (
            O::LoopsCross {
                face: Some(named),
                crossing: LoopCrossing::SharesEdge,
            },
            Some(7),
            R::LoopsCross {
                crossing: LoopCrossing::SharesEdge,
            },
        ),
        (
            O::DistanceUnresolved {
                distance: 1e-9,
                resolution: 1e-6,
            },
            None,
            R::DistanceUnresolved {
                distance: 1e-9,
                resolution: 1e-6,
            },
        ),
        (
            O::ToleranceUnresolved { tolerance: 1e-13 },
            None,
            R::ToleranceUnresolved { tolerance: 1e-13 },
        ),
        (O::SeamOpened { face: named }, Some(7), R::SeamOpened),
        (O::ExactCurveUncut, None, R::ExactCurveUncut),
        (O::FacesApart { face: other }, None, R::FacesApart),
        (
            O::ReachUncertified { face: named },
            Some(7),
            R::ReachUncertified,
        ),
        (
            O::ReachUncertified { face: other },
            None,
            R::ReachUncertified,
        ),
        (
            O::LoopGapUncertified { face: Some(named) },
            Some(7),
            R::LoopGapUncertified,
        ),
        (
            O::LoopGapUncertified { face: None },
            None,
            R::LoopGapUncertified,
        ),
        (
            O::Unsound {
                what: K::Check {
                    failures: 2,
                    first: Some((1, 4)),
                },
            },
            None,
            R::Unsound {
                what: Unsoundness::Check {
                    failures: 2,
                    first: Some((1, 4)),
                },
            },
        ),
        (
            O::Unsound {
                what: K::VolumeUndecided,
            },
            None,
            R::Unsound {
                what: Unsoundness::VolumeUndecided,
            },
        ),
        // a routine's own failure, so far only a phrase: its class, whether it is the kernel's
        // fault, and the kernel's words, as it said them when it arrived as text
        (
            O::Failed("the face has no surface"),
            None,
            R::Phrase {
                class: ErrorKind::Failed,
                defect: false,
                phrase: "the face has no surface".to_owned(),
            },
        ),
        (
            O::Missing("the face's surface"),
            None,
            R::Phrase {
                class: ErrorKind::Missing,
                defect: false,
                phrase: "the face's surface is missing".to_owned(),
            },
        ),
        (
            O::InvalidValue("the face has no outer face use"),
            None,
            R::Phrase {
                class: ErrorKind::InvalidInput,
                defect: false,
                phrase: "invalid input: the face has no outer face use".to_owned(),
            },
        ),
        (
            O::Inconsistent("an offset edge has no curve"),
            None,
            R::Phrase {
                class: ErrorKind::Failed,
                defect: true,
                phrase: "a consistency check failed: an offset edge has no curve".to_owned(),
            },
        ),
    ];
    for (o, face, why) in cases {
        let got = refused(o);
        assert_eq!(got, Error::OffsetRefused { face, why }, "{o:?}");
        assert!(!got.to_string().contains("status"), "{got}");
        // only a phrase arrives as one: every reason the kernel names is named here too
        let Error::OffsetRefused { why, .. } = &got else {
            panic!("not an offset refusal: {o:?}");
        };
        assert_eq!(matches!(why, R::Phrase { .. }), o.is_phrase(), "{o:?}");
    }
    // walls meeting across a square boss name the largest wall, not a radius of curvature
    let across = R::WallsMeetAcross {
        distance: 1.0,
        largest: 1.0,
    };
    assert_eq!(
        across.to_string(),
        "the wall reaches half the width between two opposite faces of a boss; the largest wall \
         that fits is 1"
    );
    // the face the kernel sets on a failure whose reason names none (a draft's face without a
    // surface) is the one said
    let what = "the face has no surface or no outer face use";
    let at = Failure::new("test", O::InvalidValue(what))
        .at(solidcore::EntityRef::Face(named))
        .into_error();
    assert_eq!(
        super::status(at, &index),
        Error::OffsetRefused {
            face: Some(7),
            why: R::Phrase {
                class: ErrorKind::InvalidInput,
                defect: false,
                phrase: format!("invalid input: {what}"),
            },
        }
    );
}

/// A reason ops does not name yet (one the kernel adds later) is said by its class and in the
/// kernel's words, never a bare status: [`super::unnamed`], the arm every unnamed reason takes,
/// keeps the kernel's class, whether it is a defect, and its own statement, and the error says
/// it.
#[test]
fn an_unnamed_offset_refusal_keeps_the_kernels_words() {
    use solidcore::error::offset::OffsetFailure;
    let o = OffsetFailure::ExactCurveUncut;
    let why = super::unnamed(&o);
    assert_eq!(
        why,
        crate::OffsetRefusal::Phrase {
            class: o.kind(),
            defect: o.is_defect(),
            phrase: o.to_string(),
        }
    );
    assert!(!o.to_string().is_empty());
    // one whose class has an error of its own (a new reason of the walls-touch, fold or
    // vanishing-edge kind) keeps taking it, as it did before it had a reason
    assert_eq!(
        super::by_kind(solidcore::error::ErrorKind::OffsetWallsTouch),
        Some(Error::WallsTouch)
    );
    assert_eq!(
        super::by_kind(solidcore::error::ErrorKind::OffsetFolds),
        Some(Error::WallFolds { largest: None })
    );
    assert_eq!(
        super::by_kind(solidcore::error::ErrorKind::OffsetEdgeVanishes),
        Some(Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::EdgeVanishes,
        })
    );
    assert_eq!(super::by_kind(o.kind()), None);
    let e = Error::OffsetRefused { face: Some(2), why };
    assert_eq!(
        e.to_string(),
        format!("the offset is refused at face 2: {o}")
    );
}

/// `refusal` is what the offset operations give for a kernel error, with any face unnamed: the
/// same routing as `status`, which agrees with it on every kind of error (an offset refusal is
/// typed, in words from its fields; an error with no offset reason is the kernel's status).
#[test]
fn a_kernel_offset_refusal_is_typed_for_a_caller_without_an_order() {
    use solidcore::error::offset::{LoopCrossing, OffsetFailure, Unsoundness};
    use solidcore::error::Failure;
    let face = solidcore::FaceId::from_index(4);
    let failed = |o| Failure::new("offset::offset_executive::crossing_loops", o).into_error();
    let crossing = LoopCrossing::ThreeAtOnePoint;
    let e = super::refusal(&failed(OffsetFailure::LoopsCross {
        face: Some(face),
        crossing,
    }));
    assert_eq!(
        e,
        Some(Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::LoopsCross { crossing }
        })
    );
    assert_eq!(
        e.map(|e| e.to_string()).as_deref(),
        Some("the offset is refused: its offset's loops cross in a way the offset cannot resolve: three loops cross at one point")
    );
    let what = Unsoundness::NotManifoldSolid;
    assert_eq!(
        super::refusal(&failed(OffsetFailure::Unsound { what })).map(|e| e.to_string()),
        Some(format!("the offset is refused: {what}"))
    );
    assert_eq!(
        what.to_string(),
        "the body built is not a manifold solid, as its input was"
    );
    assert_eq!(
        super::refusal(&failed(OffsetFailure::Folds {
            face,
            distance: 2.0,
            largest: 1.0
        })),
        Some(Error::WallFolds { largest: Some(1.0) })
    );
    assert_eq!(super::refusal(&solidcore::SolidError::sm_err("x")), None);
    // `status` and `refusal` agree: `status` is the refusal, else the kernel's status
    let errors = [
        failed(OffsetFailure::Failed("x")),
        failed(OffsetFailure::InvalidValue("x")),
        failed(OffsetFailure::Folds {
            face,
            distance: 2.0,
            largest: 1.0,
        }),
        failed(OffsetFailure::NotCertified {
            face,
            distance: -1.0,
        }),
        failed(OffsetFailure::Collapses {
            face,
            distance: -2.0,
            radius: 1.5,
        }),
        failed(OffsetFailure::LoopsCross {
            face: Some(face),
            crossing,
        }),
        failed(OffsetFailure::SeamOpened { face }),
        failed(OffsetFailure::Unsound { what }),
        failed(OffsetFailure::ExactCurveUncut),
        solidcore::SolidError::sm_err("x"),
        solidcore::SolidError::Upstream {
            operation: "offset::x",
            code: solidcore::error::SM_ERR_OFFSET_WALLS_TOUCH,
            entity: None,
        },
    ];
    for e in errors {
        let expected = super::refusal(&e).unwrap_or_else(|| crate::error::status(e.clone()));
        assert_eq!(super::status(e.clone(), &|_| None), expected, "{e:?}");
    }
}

/// A built body that fails what its input did not names the input's entity the kernel's check
/// names, by its index in the call's record order (#15035): an edge or vertex whose tolerance grew
/// past its certificate's bound comes from record edge, vertex or face `i`, with both numbers (of
/// an edge the order holds more than once, its first record edge). An entity the kernel names
/// none of, one the order does not hold, an edge or vertex of a caller without an order, or a
/// caller with no body at all leave it unnamed. The reasons that name no entity pass as the
/// kernel gives them.
#[test]
fn an_unsound_body_names_its_entity_by_record_index() {
    use crate::{OffsetRefusal as R, RecordEntity, Unsoundness};
    use solidcore::error::offset::{OffsetFailure as O, Unsoundness as K};
    use solidcore::error::{EntityRef, Failure};
    use solidcore::{EdgeId, FaceId, LoopId, VertexId};
    // record face 1 is kernel face 0, record edges 1 and 2 kernel edge 4, record vertex 2 kernel
    // vertex 3
    let order = Order {
        split_edges: 0,
        faces: vec![FaceId::from_index(2), FaceId::from_index(0)],
        edges: vec![
            EdgeId::from_index(9),
            EdgeId::from_index(4),
            EdgeId::from_index(4),
        ],
        vertices: vec![
            VertexId::from_index(1),
            VertexId::from_index(6),
            VertexId::from_index(3),
        ],
    };
    let failed = |what| Failure::new("test", O::Unsound { what }).into_error();
    let grown = |entity| K::ToleranceBeyondBound {
        entity,
        tolerance: 2e-4,
        bound: 1e-5,
    };
    let named = |face, entity| Error::OffsetRefused {
        face,
        why: R::Unsound {
            what: Unsoundness::ToleranceBeyondBound {
                entity,
                tolerance: 2e-4,
                bound: 1e-5,
            },
        },
    };
    let in_order = |what| super::status(failed(what), &super::in_order(&order));
    let edge = in_order(grown(Some(EntityRef::Edge(EdgeId::from_index(4)))));
    assert_eq!(edge, named(None, Some(RecordEntity::Edge(1))));
    assert_eq!(
        edge.to_string(),
        "the offset is refused: the tolerance at edge 1 of the input grew to 2e-4, past the 1e-5 \
         its certificate allows"
    );
    assert_eq!(
        in_order(grown(Some(EntityRef::Vertex(VertexId::from_index(3))))),
        named(None, Some(RecordEntity::Vertex(2)))
    );
    // a face is also the face the refusal is at
    assert_eq!(
        in_order(grown(Some(EntityRef::Face(FaceId::from_index(0))))),
        named(Some(1), Some(RecordEntity::Face(1)))
    );
    // none from the kernel, an edge the order does not hold, an entity no record index names
    for entity in [
        None,
        Some(EntityRef::Edge(EdgeId::from_index(7))),
        Some(EntityRef::Loop(LoopId::from_index(0))),
    ] {
        assert_eq!(in_order(grown(entity)), named(None, None), "{entity:?}");
    }
    let unnamed = named(None, None);
    assert_eq!(
        unnamed.to_string(),
        "the offset is refused: the tolerance of an edge or vertex grew to 2e-4, past the 1e-5 \
         its certificate allows"
    );
    // a caller with no body names none; one without an order names a face by its place among
    // the body's faces, and no edge or vertex
    assert_eq!(
        super::refusal(&failed(grown(Some(EntityRef::Edge(EdgeId::from_index(4)))))),
        Some(unnamed.clone())
    );
    let body = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let b = body.lock();
    let (second, first_edge) = (b.face_list()[1], b.edge_list()[0]);
    let in_list = |what| super::status(failed(what), &super::in_list(&b));
    assert_eq!(
        in_list(grown(Some(EntityRef::Face(second)))),
        named(Some(1), Some(RecordEntity::Face(1)))
    );
    assert_eq!(in_list(grown(Some(EntityRef::Edge(first_edge)))), unnamed);
    // the reasons without an entity, each as the kernel says it
    let cases = [
        (
            K::Check {
                failures: 2,
                first: Some((1, 4)),
            },
            Unsoundness::Check {
                failures: 2,
                first: Some((1, 4)),
            },
        ),
        (K::GeometryUnchecked, Unsoundness::GeometryUnchecked),
        (
            K::LoopOrientationUndecided,
            Unsoundness::LoopOrientationUndecided,
        ),
        (K::LoopGapNotClosable, Unsoundness::LoopGapNotClosable),
        (K::NotManifoldSolid, Unsoundness::NotManifoldSolid),
        (K::VolumeNotPositive, Unsoundness::VolumeNotPositive),
        (K::VolumeUndecided, Unsoundness::VolumeUndecided),
    ];
    for (kernel, ops) in cases {
        let e = in_order(kernel);
        assert_eq!(
            e,
            Error::OffsetRefused {
                face: None,
                why: R::Unsound { what: ops },
            }
        );
        assert_eq!(e.to_string(), format!("the offset is refused: {kernel}"));
    }
}

/// A face whose surface the kernel's room test cannot measure (an offset surface): the room
/// test's answer maps to `Unsupported`, not to a room or `NoRoom`.
#[test]
fn an_unmeasurable_face_is_unsupported() {
    all_fixes();
    let r = run("unmeasurable", |k| {
        let mut b = solidcore::primitive::api_primitives::create_box(
            k,
            solidcore::Vec3::ZERO,
            20.0,
            20.0,
            20.0,
        )?;
        let f = b.face_list()[0];
        let base = b
            .face(f)?
            .surface
            .clone()
            .ok_or(solidcore::SolidError::sm_err("test"))?;
        let off = solidcore::surface::offset_surface::OffsetSurface::new(k, 0.0, base, true, 0)?;
        b.face_mut(f)?.surface = Some(solidcore::surface::surface::Surface::from(off));
        super::room(k, &mut b, &[], 2.0)
    })
    .unwrap();
    assert!(matches!(r, Err(Error::Unsupported(_))), "{r:?}");
}

/// A block with every face flipped is manifold but faces inward: an inward shell and faces
/// moved in are refused (the room test would read "inward" outward), the body as it came in.
#[test]
fn an_inside_out_body_is_refused() {
    all_fixes();
    let b = make_box([0.0; 3], [20.0, 20.0, 20.0]).unwrap();
    {
        let mut br = b.lock();
        for f in br.face_list().to_vec() {
            solidcore::topology::face::p1::flip_face_orientation(&mut br, f).unwrap();
        }
    }
    let o = order(&b);
    let before = b.to_smb(false).unwrap();
    assert_eq!(shell(&b, &o, &[], -15.0), Err(Error::Argument));
    assert_eq!(offset_faces(&b, &o, &[0], -2.0), Err(Error::Argument));
    assert_eq!(b.to_smb(false).unwrap(), before);
}

/// A 90.5 x 52 x 50 box less a tunnel lofted (degree 2) from a circle r 16 about (4, -5) on its
/// floor, through a circle r 13.5 on the plane through its edges x = 69, z = 0 and
/// x = -21.5, z = 50, to a circle r 11.07 about y = -4.5, z = 34 on its side x = 69: the body
/// of #14062, the sections as SolidCraft traced them. The loft face's seam runs from the
/// floor circle's one vertex, (4, 11, 0), to the side circle's.
fn box_with_a_lofted_tunnel() -> Body {
    let circle =
        |center: [f64; 3], axis: [f64; 3], reference: [f64; 3], radius: f64, start: f64| {
            vec![crate::sweep::Curve::Arc {
                center,
                axis,
                reference,
                radius,
                start,
                end: start + 2.0 * PI,
            }]
        };
    let tunnel = crate::sweep::loft(
        &[
            circle(
                [4.0, -5.0, 0.0],
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                16.0,
                -PI / 2.0,
            ),
            circle(
                [27.538758280222268, -5.747400290549194, 22.90676338109267],
                [-0.48358862132842667, 0.0, -0.8752954046044522],
                [0.0, 1.0, 0.0],
                13.503737308981044,
                -2.0 * PI,
            ),
            circle(
                [69.0, -4.5, 34.0],
                [-1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                11.067971810589327,
                -2.0 * PI,
            ),
        ],
        2,
        true,
    )
    .unwrap();
    let block = make_box([-21.5, -30.5, 0.0], [90.5, 52.0, 50.0]).unwrap();
    crate::boolean::boolean(block, tunnel, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// #14062: the box with a lofted tunnel shelled t inward, open at its side x = 69. The
/// tunnel's floor edge is closed, its one vertex where the loft's seam meets the floor, and
/// the offsets of the loft face and the floor meet on a closed curve whose seam is that
/// vertex's offset: the inner floor's hole edge. Shelled, the body is the outer box and
/// tunnel (6 planes, the side split into a frame and a ring round the tunnel, and the loft
/// face) and the inner box and tunnel (5 planes and the loft's offset): 14 faces; 12 + 12
/// box edges and 3 + 3 tunnel edges (two circles and a seam each): 30 edges; 8 + 8 corners
/// and 2 + 2 seam ends: 20 vertices; one shell. The hole edge lies on z = t and at t from
/// the loft face wherever the planes x = c cross it. SMLib's full check is
/// [`a_box_with_a_lofted_tunnel_shelled_passes_the_full_check`]'s.
#[test]
fn a_box_with_a_lofted_tunnel_shells_open_at_its_side() {
    use solidcore::query::api_queries::{edge_bounding_box, edge_plane_crossings, PlaneCrossing};
    use solidcore::{Point3, Vec3};
    all_fixes();
    for t in [0.5, 1.0, 2.0] {
        let b = box_with_a_lofted_tunnel();
        let o = order(&b);
        // the loft face, the one face not flat along an axis
        let loft = face(&b, &o, |bx| {
            let size = bx.max() - bx.min();
            size.x > 1.0 && size.y > 1.0 && size.z > 1.0
        });
        let input = b.copy().unwrap();
        let input_order = order(&input);
        shell(&b, &o, &[through(&b, &o, [69.0, -25.0, 10.0])], -t).unwrap();
        assert_eq!(b.counts().unwrap(), [14, 30, 20], "t {t}");
        assert_eq!(b.to_record().unwrap().0.shells.len(), 1, "t {t}");
        assert!(b.is_manifold_solid().unwrap());
        b.check_solid().unwrap();

        let k = Kernel::default();
        let brep = b.lock();
        let hole: Vec<_> = brep
            .edge_list()
            .iter()
            .copied()
            .filter(|&e| {
                let bx = edge_bounding_box(&k, &brep, e, true).unwrap();
                let size = bx.max() - bx.min();
                size.x > 1.0 && size.y > 1.0 && (bx.min().z - t).abs() < 1e-3 && size.z < 1e-3
            })
            .collect();
        assert_eq!(hole.len(), 1, "t {t}: inner floor hole edges {hole:?}");
        let tol = brep.edge(hole[0]).unwrap().tolerance + brep.tolerance();
        for c in [-8.0, 4.0, 16.0] {
            let crossings = edge_plane_crossings(
                &k,
                &brep,
                hole[0],
                Point3::new(c, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            )
            .unwrap();
            assert_eq!(crossings.len(), 2, "t {t}, x = {c}: {crossings:?}");
            for crossing in crossings {
                let PlaneCrossing::Point(at) = crossing else {
                    panic!("t {t}, x = {c}: the edge runs along the plane");
                };
                let p = [at.point.x, at.point.y, at.point.z];
                assert!((p[2] - t).abs() <= tol, "t {t}: {p:?} off z = t");
                let d = input.face_distances(&input_order, loft, &[p]).unwrap()[0].unwrap();
                assert!(
                    (d - t).abs() <= tol,
                    "t {t}: {p:?} at {d} from the loft face"
                );
            }
        }
    }
}

/// #14062: the box with a lofted tunnel shelled t inward with its floor (z = 0) or its top
/// (z = 50) open instead of its side. The tunnel's floor edge mates as with the side open.
/// Floor open: the side keeps its offset ring round the tunnel, 14 faces as with the side
/// open; top open: the top has no hole to split it, 13 faces. Both 30 edges, 20 vertices,
/// one shell.
#[test]
fn a_box_with_a_lofted_tunnel_shells_open_at_its_floor_or_top() {
    all_fixes();
    for (open, faces) in [([60.0, -25.0, 0.0], 14), ([60.0, -25.0, 50.0], 13)] {
        for t in [1.0, 2.0] {
            let b = box_with_a_lofted_tunnel();
            let o = order(&b);
            shell(&b, &o, &[through(&b, &o, open)], -t).unwrap();
            assert_eq!(b.counts().unwrap(), [faces, 30, 20], "{open:?} t {t}");
            assert_eq!(b.to_record().unwrap().0.shells.len(), 1, "{open:?} t {t}");
            assert!(b.is_manifold_solid().unwrap(), "{open:?} t {t}");
            b.check_solid().unwrap();
        }
    }
}

/// #17608: the shelled box with a lofted tunnel of
/// [`a_box_with_a_lofted_tunnel_shells_open_at_its_side`] passes SMLib's full check. It
/// fails test #36 ("Face Near missing Seam"): the offset loft face's seam edge ends 4e-9 of
/// the loft's turn beside the seam at the floor vertex.
#[test]
#[ignore = "fails until #17608 is fixed"]
fn a_box_with_a_lofted_tunnel_shelled_passes_the_full_check() {
    all_fixes();
    for t in [0.5, 1.0, 2.0] {
        let b = box_with_a_lofted_tunnel();
        let o = order(&b);
        shell(&b, &o, &[through(&b, &o, [69.0, -25.0, 10.0])], -t).unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "t {t}: {}", report.text);
    }
}

/// The rounded rectangle `x` in [x0, x1], `z` in [z0, z1] with corners of radius 8 on the plane
/// y = `y`, counter-clockwise about -y from its bottom side's start, as SolidCraft places a
/// sketch on x = (1, 0, 0), y = (0, 0, 1).
fn rounded_section(x0: f64, x1: f64, z0: f64, z1: f64, y: f64) -> Vec<crate::sweep::Curve> {
    use crate::sweep::Curve;
    let r = 8.0;
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], y, a[1]],
        end: [b[0], y, b[1]],
    };
    let arc = |c: [f64; 2], start: f64| Curve::Arc {
        center: [c[0], y, c[1]],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start,
        end: start + PI / 2.0,
    };
    vec![
        line([x0 + r, z0], [x1 - r, z0]),
        arc([x1 - r, z0 + r], -PI / 2.0),
        line([x1, z0 + r], [x1, z1 - r]),
        arc([x1 - r, z1 - r], 0.0),
        line([x1 - r, z1], [x0 + r, z1]),
        arc([x0 + r, z1 - r], PI / 2.0),
        line([x0, z1 - r], [x0, z0 + r]),
        arc([x0 + r, z0 + r], PI),
    ]
}

/// #21215's sections: the tube's, x in [0, 100], z in [0, 100], and the flare's far end,
/// x in [-28, 100], z in [0, 128], both with corners of radius 8.
const TUBE: [f64; 4] = [0.0, 100.0, 0.0, 100.0];
const FLARE: [f64; 4] = [-28.0, 100.0, 0.0, 128.0];

/// #21215's flare: the ruled loft from [`TUBE`]'s section at y = -80 to [`FLARE`]'s at y = -150.
fn flare() -> Body {
    let [a, b, c, d] = TUBE;
    let [e, f, g, h] = FLARE;
    crate::sweep::loft(
        &[
            rounded_section(a, b, c, d, -80.0),
            rounded_section(e, f, g, h, -150.0),
        ],
        1,
        true,
    )
    .unwrap()
}

/// #21215: [`TUBE`]'s section extruded from y = 0 to y = -80, joined to [`flare`]: one body,
/// the round corners of the tube and the flare meeting at the crease y = -80.
fn tube_joined_to_a_flare() -> Body {
    let [a, b, c, d] = TUBE;
    let tube = crate::sweep::extrude(
        &rounded_section(a, b, c, d, 0.0),
        [0.0, -1.0, 0.0],
        80.0,
        0.0,
        crate::sweep::DraftCorner::Curvature,
        true,
    )
    .unwrap();
    crate::boolean::boolean(tube, flare(), crate::BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// The volume of the part of the tube's erosion by `t` (its inset section, swept along y) past
/// y = -80 that lies outside the flare's erosion: per slice y, the inset section's area outside
/// the flare's erosion section, integrated by Simpson's rule from y = -80 to y = -80 - 0.4 t
/// (past every crease's wedge, t tan(lean / 2) deep: under 0.27 t for the steepest lean,
/// atan(0.4 sqrt 2), where the round between the two flared sides leans both ways). Both
/// are convex: a section of an erosion is the intersection of the half-planes the supporting
/// planes moved in by t cut from it, here those at the outward normals of 65 angles a quarter
/// turn (each straight side's among them). A ruling of the flare joins its sections' points of
/// one outward normal angle phi, and its supporting plane's normal is (cos phi, a, sin phi),
/// square to the ruling.
#[allow(clippy::disallowed_methods)] // the oracle's own arithmetic, not ops geometry
fn erosion_wedge(t: f64) -> f64 {
    const STEPS: usize = 64;
    const SLICES: usize = 64;
    // the point of section `s` (x0, x1, z0, z1) whose outward normal is at angle phi
    let support = |s: [f64; 4], phi: f64| -> [f64; 2] {
        let (sin, cos) = phi.sin_cos();
        let pick = |c: f64, lo: f64, hi: f64| {
            if c > 1e-12 {
                hi - 8.0
            } else if c < -1e-12 {
                lo + 8.0
            } else {
                (lo + hi) / 2.0
            }
        };
        [
            pick(cos, s[0], s[1]) + 8.0 * cos,
            pick(sin, s[2], s[3]) + 8.0 * sin,
        ]
    };
    let angles: Vec<f64> = (0..4)
        .flat_map(|q| (0..=STEPS).map(move |i| (q * STEPS + i) as f64 * PI / (2 * STEPS) as f64))
        .collect();
    // the half-planes a x + b z <= c
    let tube: Vec<[f64; 3]> = angles
        .iter()
        .map(|&phi| {
            let p = support(TUBE, phi);
            [
                phi.cos(),
                phi.sin(),
                phi.cos() * p[0] + phi.sin() * p[1] - t,
            ]
        })
        .collect();
    let flare_at = |y: f64| -> Vec<[f64; 3]> {
        angles
            .iter()
            .map(|&phi| {
                let (p, q) = (support(TUBE, phi), support(FLARE, phi));
                let (cos, sin) = (phi.cos(), phi.sin());
                let a = (cos * (q[0] - p[0]) + sin * (q[1] - p[1])) / 70.0;
                let n = (1.0 + a * a).sqrt();
                let at = (cos * p[0] + a * -80.0 + sin * p[1]) / n - t - a / n * y;
                [cos / n, sin / n, at]
            })
            .collect()
    };
    let clip = |poly: Vec<[f64; 2]>, [a, b, c]: [f64; 3]| -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for (k, &p) in poly.iter().enumerate() {
            let q = poly[(k + 1) % poly.len()];
            let (fp, fq) = (a * p[0] + b * p[1] - c, a * q[0] + b * q[1] - c);
            if fp <= 0.0 {
                out.push(p);
            }
            if (fp < 0.0 && fq > 0.0) || (fq < 0.0 && fp > 0.0) {
                let u = fp / (fp - fq);
                out.push([p[0] + u * (q[0] - p[0]), p[1] + u * (q[1] - p[1])]);
            }
        }
        out
    };
    let shoelace = |poly: &[[f64; 2]]| -> f64 {
        (0..poly.len())
            .map(|k| {
                let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
                p[0] * q[1] - q[0] * p[1]
            })
            .sum::<f64>()
            / 2.0
    };
    let big = vec![[-1e3, -1e3], [1e3, -1e3], [1e3, 1e3], [-1e3, 1e3]];
    let inset = tube.iter().fold(big, |poly, &h| clip(poly, h));
    let outside = |y: f64| {
        let both = flare_at(y).into_iter().fold(inset.clone(), |poly, h| {
            if poly.is_empty() {
                poly
            } else {
                clip(poly, h)
            }
        });
        shoelace(&inset) - shoelace(&both)
    };
    let h = 0.4 * t / SLICES as f64;
    (0..=SLICES)
        .map(|k| {
            let weight = if k == 0 || k == SLICES {
                1.0
            } else if k % 2 == 1 {
                4.0
            } else {
                2.0
            };
            weight * outside(-80.0 - k as f64 * h)
        })
        .sum::<f64>()
        * h
        / 3.0
}

/// #21215: [`tube_joined_to_a_flare`] shelled t inward, open at both ends (y = 0 and
/// y = -150), is a valid solid. At the crease the flare's top and its side x = 0 lean out
/// atan 0.4 from the tube's, and the round between them runs along (-28, -70, 28): the
/// offsets of the tube's round and side and of the flare's round and side, each pair tangent,
/// cross the tube's side's offset at two points, the flare's round's offset meeting it along
/// a curve between them. Each wall is convex, so its inward offset is its erosion, and the
/// cavity at the crease, which is concave from outside, is the union of the two. The
/// material is then the tube's shell (open at y = 0, with a wall t thick at y = -80) and the
/// flare's (open at both ends), less that wall (t times the tube's inset section) and less
/// the part of the tube's erosion past y = -80 outside the flare's ([`erosion_wedge`]).
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_tube_joined_to_a_flared_loft_shells_open_at_both_ends() {
    all_fixes();
    let inset = |t: f64| (100.0 - 2.0 * t).powi(2) - (4.0 - PI) * (8.0 - t).powi(2);
    for t in [2.0, 6.0] {
        let b = tube_joined_to_a_flare();
        let o = order(&b);
        let ends = [
            through(&b, &o, [50.0, 0.0, 50.0]),
            through(&b, &o, [50.0, -150.0, 50.0]),
        ];
        shell(&b, &o, &ends, -t).unwrap_or_else(|e| panic!("t {t}: {e:?}"));
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "t {t}: {}", report.text);

        let alone = flare();
        let fo = order(&alone);
        let flare_ends = [
            through(&alone, &fo, [50.0, -80.0, 50.0]),
            through(&alone, &fo, [50.0, -150.0, 50.0]),
        ];
        shell(&alone, &fo, &flare_ends, -t).unwrap();
        let tube = 80.0 * inset(0.0) - (80.0 - t) * inset(t);
        let want = tube + volume(&alone) - t * inset(t) - erosion_wedge(t);
        assert!(
            rel(volume(&b), want) < 1e-7,
            "t {t}: {} vs {want}",
            volume(&b)
        );
    }
}

/// Drafts against closed forms (SolidCraft's `draft_*` cases, on record face indices), the oblique
/// neutral plane's, and the refusals.
mod drafts {
    use super::{all_fixes, block, face, order, rel, top, volume, ABC};
    use crate::offset::{draft, mark_moved, moved_faces};
    use crate::{make_box, make_cone, make_cylinder, Error, Order};
    use solidcore::geom::attribute::{ids, Attribute, Behavior};
    use solidcore::EntityRef;
    use std::f64::consts::PI;

    const Z: [f64; 3] = [0.0, 0.0, 1.0];

    // The closed forms' angles and tangents as numbers (ops computes no trigonometry, its
    // tests included): each the double nearest its expression.
    /// 5 degrees, radians.
    const FIVE: f64 = 0.087_266_462_599_716_47;
    /// tan 5 degrees.
    const TAN_FIVE: f64 = 0.087_488_663_525_924_01;
    /// 10 degrees, radians.
    const TEN: f64 = 0.174_532_925_199_432_95;
    /// tan 10 degrees.
    const TAN_TEN: f64 = 0.176_326_980_708_464_98;
    /// tan 0.1.
    const TAN_TENTH: f64 = 0.100_334_672_085_450_55;
    /// tan(phi), sin(phi) = sin(5 degrees) / 0.8: the turn about a hinge at sin(alpha) = 0.8
    /// to the pull that leaves a face at 5 degrees to it.
    const TAN_OBLIQUE: f64 = 0.109_597_020_255_368_76;
    /// tan(atan(-0.1) - 5 degrees).
    const TAN_CONE: f64 = -0.189_143_454_329_318_9;

    fn five() -> f64 {
        FIVE
    }

    /// The record index (through `o`) of the block's side flat at `x = at` (`axis` 0) or
    /// `y = at` (1).
    fn side(b: &crate::Body, o: &Order, axis: usize, at: f64) -> usize {
        face(b, o, |bx| {
            let c = |p: solidcore::Vec3| [p.x, p.y][axis];
            (c(bx.min()) - at).abs() < 1e-9 && (c(bx.max()) - at).abs() < 1e-9
        })
    }

    /// The record index (through `o`) of the face of `b` that is no plane: its tight box is
    /// not flat on any axis.
    fn curved(b: &crate::Body, o: &Order) -> usize {
        face(b, o, |bx| {
            let (lo, hi) = (bx.min(), bx.max());
            hi.x - lo.x > 1e-6 && hi.y - lo.y > 1e-6 && hi.z - lo.z > 1e-6
        })
    }

    /// The four sides drafted 5 degrees about the bottom, pulled up, leaning in: the section
    /// at height z is (a - 2 z s)(b - 2 z s), s = tan 5 degrees, so
    /// V = abh - (a + b) h^2 s + (4/3) h^3 s^2.
    #[test]
    fn the_sides_of_a_block() {
        all_fixes();
        let b = block();
        let o = order(&b);
        let sides = [
            side(&b, &o, 0, 0.0),
            side(&b, &o, 0, 30.0),
            side(&b, &o, 1, 0.0),
            side(&b, &o, 1, 20.0),
        ];
        draft(&b, &o, &sides, [0.0; 3], Z, Z, five()).unwrap();
        let (a, w, h, s) = (30.0, 20.0, 10.0, TAN_FIVE);
        let v = a * w * h - (a + w) * h * h * s + 4.0 / 3.0 * h * h * h * s * s;
        assert!(rel(volume(&b), v) < 1e-9, "{} {v}", volume(&b));
        assert_eq!(b.counts().unwrap()[0], 6);
    }

    /// The front (y = 0) alone: a wedge of a h^2 tan(5 degrees) / 2 comes off; a negative angle
    /// puts it on; hinged at the top, leaning in going up, the face leans out below the hinge
    /// and the wedge goes on.
    #[test]
    fn one_side_in_out_and_hinged_at_the_top() {
        all_fixes();
        let wedge = 30.0 * 100.0 * TAN_FIVE / 2.0;
        for (origin, angle, v) in [
            ([0.0; 3], five(), ABC - wedge),
            ([0.0; 3], -five(), ABC + wedge),
            ([0.0, 0.0, 10.0], five(), ABC + wedge),
        ] {
            let b = block();
            let o = order(&b);
            draft(&b, &o, &[side(&b, &o, 1, 0.0)], origin, Z, Z, angle).unwrap();
            assert!(
                rel(volume(&b), v) < 1e-9,
                "{origin:?} {angle}: {}",
                volume(&b)
            );
        }
    }

    /// The rig's two drafts (booleans/cov-draft-two-adjacent-faces-meet-at-a-corner, and the
    /// draft step of fillets/4989-chamfer-top-edge-of-drafted-face): the 40 x 30 x 20 block's
    /// front, and its front and right, drafted 10 degrees about its base. The section at height
    /// z is (40 - z t)(30 - z t), t = tan 10 degrees, for both, (40)(30 - z t) for the front;
    /// six faces either way.
    #[test]
    fn the_rigs_drafts() {
        all_fixes();
        let (ten, t) = (TEN, TAN_TEN);
        let both = 24000.0 - 70.0 * t * 200.0 + t * t * 8000.0 / 3.0;
        let front = 24000.0 - 0.5 * 20.0 * (20.0 * t) * 40.0;
        for (right, v) in [(true, both), (false, front)] {
            let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
            let o = order(&b);
            let mut faces = vec![side(&b, &o, 1, 0.0)];
            if right {
                faces.push(side(&b, &o, 0, 40.0));
            }
            draft(&b, &o, &faces, [0.0; 3], Z, Z, ten).unwrap();
            let got = volume(&b);
            println!("rig draft (right {right}): volume {got:.17e}, closed form {v:.17e}");
            assert!(rel(got, v) < 1e-12, "{got} {v}");
            assert_eq!(b.counts().unwrap()[0], 6);
        }
    }

    /// The neutral plane oblique to the pull, normal N = (0, 0.6, 0.8) through the origin: the
    /// side x = 30 turns about the line where it meets it (x = 30, 0.6 y + 0.8 z = 0), which is
    /// at sin(alpha) = 0.8 to the pull, by phi with sin(phi) = sin(5 degrees) / 0.8, so that it
    /// ends at exactly 5 degrees to the pull: the plane x = 30 - tan(phi) (0.6 y + 0.8 z), so
    /// what comes off is tan(phi) times the integral of 0.6 y + 0.8 z over the side,
    /// 1200 + 800.
    #[test]
    fn an_oblique_neutral_plane_hinges_on_its_trace() {
        all_fixes();
        let b = block();
        let o = order(&b);
        draft(
            &b,
            &o,
            &[side(&b, &o, 0, 30.0)],
            [0.0; 3],
            [0.0, 0.6, 0.8],
            Z,
            five(),
        )
        .unwrap();
        let v = ABC - 2000.0 * TAN_OBLIQUE;
        assert!(rel(volume(&b), v) < 1e-9, "{} {v}", volume(&b));
        // a draft steeper than the hinge allows (sin 10 degrees > 0.141) is a typed refusal
        let b = block();
        let o = order(&b);
        assert!(matches!(
            draft(
                &b,
                &o,
                &[side(&b, &o, 0, 30.0)],
                [0.0; 3],
                [0.0, 0.99, 0.141],
                Z,
                TEN
            ),
            Err(Error::Unsupported(_))
        ));
    }

    /// A cylinder along the pull, R 5, h 10, drafted 0.1 about its bottom, leaning in: the
    /// frustum of radii R and R - h tan 0.1.
    #[test]
    fn a_cylinder_drafts_to_a_frustum() {
        all_fixes();
        let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let o = order(&c);
        draft(&c, &o, &[curved(&c, &o)], [0.0; 3], Z, Z, 0.1).unwrap();
        let (big, h) = (5.0, 10.0);
        let small = big - h * TAN_TENTH;
        let v = PI * h * (big * big + big * small + small * small) / 3.0;
        assert!(rel(volume(&c), v) < 1e-9, "{} {v}", volume(&c));
    }

    /// Acid-6's needle: R = 0.5, h = 1000, neutral at z = -1600, theta = 0.02 degrees.
    /// r(z) = R - (z + 1600) tan(theta), so the frustum volume is
    /// `pi h (R^2 + R r_top + r_top^2) / 3` = 364.684642297274456 mm^3;
    /// `r_top` = 0.150934135423594902 mm.
    #[test]
    fn a_long_thin_cylinder_drafts_to_a_frustum() {
        all_fixes();
        let base = [0.0, 0.0, -1600.0];
        let c = make_cylinder(base, 0.5, 1000.0).unwrap();
        let o = order(&c);
        // 0.02 pi / 180 and tan(0.02 pi / 180), rounded to doubles.
        let angle = 0.000_349_065_850_398_865_93;
        let slope = 0.000_349_065_864_576_405_1;
        draft(&c, &o, &[curved(&c, &o)], base, Z, Z, angle).unwrap();
        let small = 0.5 - 1000.0 * slope;
        let want = PI * 1000.0 * (0.25 + 0.5 * small + small * small) / 3.0;
        let got = volume(&c);
        assert!(rel(got, want) < 1e-9, "{got} {want}");
        assert!(c.is_manifold_solid().unwrap());
        assert_eq!(c.check(crate::CheckLevel::All).unwrap().failures, 0);
        assert_eq!(c.counts().unwrap(), [3, 3, 2]);
        // At z = -601 the radius is about 0.1513; at -1599 it is about 0.4997.
        for (point, class) in [
            ([0.0, 0.0, -601.0], crate::PointClass::Inside),
            ([0.2, 0.0, -601.0], crate::PointClass::Outside),
            ([0.2, 0.0, -1599.0], crate::PointClass::Inside),
            ([0.0, 0.0, -1601.0], crate::PointClass::Outside),
            ([0.0, 0.0, -599.0], crate::PointClass::Outside),
        ] {
            assert_eq!(c.classify_point(point).unwrap(), class, "{point:?}");
        }
    }

    /// A cone r 10 at its base to 8 at its top, 20 tall, drafted 5 degrees about its base turns
    /// its generators 5 degrees more: r 10 + 20 tan(atan(-0.1) - 5 degrees) at the top.
    #[test]
    fn a_cone_about_the_pull_turns_its_generators() {
        all_fixes();
        let c = make_cone([0.0; 3], 10.0, 8.0, 20.0).unwrap();
        let o = order(&c);
        draft(&c, &o, &[curved(&c, &o)], [0.0; 3], Z, Z, five()).unwrap();
        let (big, h) = (10.0, 20.0);
        let small = big + h * TAN_CONE;
        let v = PI * h * (big * big + big * small + small * small) / 3.0;
        assert!(rel(volume(&c), v) < 1e-9, "{} {v}", volume(&c));
    }

    /// Refused before the kernel or by its draft, the body as it came in: no neutral normal, no
    /// angle, a right angle, an origin not finite or past 1e9, no faces, an index past the faces
    /// or twice, the pull in the neutral plane (Argument); the top across the pull (Unsupported).
    #[test]
    fn hostile_drafts_are_refused() {
        all_fixes();
        let b = block();
        let o = order(&b);
        let before = b.to_smb(false).unwrap();
        let t = top(&b, &o);
        let s = side(&b, &o, 1, 0.0);
        for (faces, at, n, p, angle) in [
            (vec![s], [0.0; 3], [0.0; 3], Z, 0.1),
            (vec![s], [0.0; 3], Z, [0.0; 3], 0.1),
            (vec![s], [0.0; 3], Z, Z, 0.0),
            (vec![s], [0.0; 3], Z, Z, f64::NAN),
            (vec![s], [0.0; 3], Z, Z, PI / 2.0),
            (vec![s], [0.0; 3], Z, Z, -1.569_796_326_794_896_6),
            (vec![s], [f64::NAN; 3], Z, Z, 0.1),
            (vec![s], [0.0, 2e9, 0.0], Z, Z, 0.1),
            (vec![], [0.0; 3], Z, Z, 0.1),
            (vec![6], [0.0; 3], Z, Z, 0.1),
            (vec![s, s], [0.0; 3], Z, Z, 0.1),
            (vec![s], [0.0; 3], [1.0, 0.0, 0.0], Z, 0.1),
        ] {
            assert_eq!(
                draft(&b, &o, &faces, at, n, p, angle),
                Err(Error::Argument),
                "{faces:?} {at:?} {n:?} {p:?} {angle}"
            );
        }
        assert!(matches!(
            draft(&b, &o, &[t], [0.0; 3], Z, Z, 0.1),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(b.to_smb(false).unwrap(), before);
        // just under the largest angle passes the argument check
        let other = block();
        assert_ne!(
            draft(
                &other,
                &order(&other),
                &[s],
                [0.0; 3],
                Z,
                Z,
                1.569_796_326_794_896
            ),
            Err(Error::Argument)
        );
    }

    /// A body facing in is no solid to draft: refused, as it came in.
    #[test]
    fn an_inside_out_body_is_not_drafted() {
        all_fixes();
        let b = make_box([0.0; 3], [20.0, 20.0, 20.0]).unwrap();
        {
            let mut br = b.lock();
            for f in br.face_list().to_vec() {
                solidcore::topology::face::p1::flip_face_orientation(&mut br, f).unwrap();
            }
        }
        let o = order(&b);
        let before = b.to_smb(false).unwrap();
        assert_eq!(
            draft(&b, &o, &[0], [0.0; 3], Z, Z, 0.1),
            Err(Error::Argument)
        );
        assert_eq!(b.to_smb(false).unwrap(), before);
    }

    /// The moved marks live inside the call: a face marked reads as moved once and the mark is
    /// gone; an unmarked body reads as not moved (the draft's `NoEffect`); a drafted body keeps
    /// no draft mark, and a mark the body already carried (the kernel's `ids::MARK`) stays.
    #[test]
    fn the_moved_marks_live_inside_the_call() {
        all_fixes();
        let b = block();
        {
            let mut br = b.lock();
            assert!(!moved_faces(&mut br).unwrap(), "no face marked");
            let f = br.face_list()[0];
            mark_moved(&mut br, &[f]).unwrap();
            assert!(moved_faces(&mut br).unwrap(), "a face marked");
            assert!(!moved_faces(&mut br).unwrap(), "the mark removed");
        }
        let marks = |b: &crate::Body| -> Vec<Option<i64>> {
            let br = b.lock();
            br.face_list()
                .iter()
                .flat_map(|&f| {
                    br.entity_attributes(EntityRef::Face(f))
                        .into_iter()
                        .filter(|(_, a)| [ids::MARK, ids::DRAFT_MOVED].contains(&a.attribute_id()))
                        .map(|(_, a)| a.long_value())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let o = order(&b);
        let s = side(&b, &o, 1, 0.0);
        {
            let mut br = b.lock();
            let f = o.faces[s];
            br.add_attribute(
                EntityRef::Face(f),
                Attribute::long(ids::MARK, 7, Behavior::Copy),
            )
            .unwrap();
        }
        draft(&b, &o, &[s], [0.0; 3], Z, Z, five()).unwrap();
        assert_eq!(marks(&b), vec![Some(7)]);
        let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let o = order(&c);
        draft(&c, &o, &[curved(&c, &o)], [0.0; 3], Z, Z, 0.1).unwrap();
        assert!(marks(&c).is_empty());
    }

    /// 3 degrees, radians.
    const THREE: f64 = 0.052_359_877_559_829_89;
    /// tan 3 degrees.
    const TAN_THREE: f64 = 0.052_407_779_283_041_21;

    /// A boss r 10 about (20, 20), z 10 to 20, drawn as two faces: a circle extruded as two
    /// half arcs, so its wall is split along the rulings through (10, 20) and (30, 20) and its
    /// top has a vertex on each, where the two wall faces and the top meet.
    fn split_boss(on_block: bool) -> crate::Body {
        use crate::prism::extrude_planar_regions;
        use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
        let half = |start: f64| Seg2::Arc {
            center: Vec2::new(20.0, 20.0),
            radius: 10.0,
            start,
            sweep: PI,
        };
        let disc = Region2 {
            outer: Loop2 {
                segs: vec![half(0.0), half(PI)],
            },
            holes: vec![],
        };
        let boss = extrude_planar_regions(&Plane::XY, &[disc], 10.0, 20.0).unwrap();
        if !on_block {
            return boss;
        }
        let block = make_box([0.0; 3], [40.0, 40.0, 10.0]).unwrap();
        crate::boolean::boolean(block, boss, crate::BooleanOp::Union)
            .unwrap()
            .unwrap()
    }

    /// The top of a split boss (alone, and standing on a 40 x 40 x 10 block) drafted 3 degrees
    /// about the line x = 25 on it, pulled along x, leaning in going along x: the top turns
    /// to z = 20 - (x - 25) tan 3 degrees, 5 tan 3 degrees above z = 20 at the axis, so the boss
    /// gains pi r^2 5 tan 3 degrees; leaning out it loses as much. The top's vertices on the
    /// split rulings meet the turned top where the rulings cross it (#3005 refused them: two
    /// faces of one cylinder at a vertex where a plane moves). Valid at every check level.
    ///
    /// The split wall's halves are extended past their rulings, so the top's new edges are
    /// fitted, within the B-rep's tolerance (2e-5) of the ellipse: the volume is held to that
    /// tolerance over the top's area (under 315). The same boss drawn as one face with a seam
    /// keeps an exact ellipse and lands on the closed form to rounding.
    #[test]
    fn the_top_of_a_boss_split_along_its_rulings() {
        all_fixes();
        let x = [1.0, 0.0, 0.0];
        let band = 2e-5 * 315.0;
        for (split, on_block) in [(true, false), (true, true), (false, false)] {
            for (angle, sign) in [(THREE, 1.0), (-THREE, -1.0)] {
                let b = if split {
                    split_boss(on_block)
                } else {
                    make_cylinder([20.0, 20.0, 10.0], 10.0, 10.0).unwrap()
                };
                let before = volume(&b);
                let o = order(&b);
                let top = face(&b, &o, |bx| {
                    (bx.min().z - 20.0).abs() < 1e-9 && (bx.max().z - 20.0).abs() < 1e-9
                });
                draft(&b, &o, &[top], [25.0, 0.0, 0.0], x, x, angle).unwrap();
                let want = before + sign * PI * 10.0 * 10.0 * 5.0 * TAN_THREE;
                let got = volume(&b);
                if split {
                    assert!(
                        (got - want).abs() < band,
                        "{on_block} {angle}: {got} {want}"
                    );
                } else {
                    assert!(rel(got, want) < 1e-9, "seam {angle}: {got} {want}");
                }
                let report = b.check(crate::CheckLevel::All).unwrap();
                assert_eq!(
                    report.failures, 0,
                    "{split} {on_block} {angle}: {}",
                    report.text
                );
            }
        }
    }

    /// A cylinder of radius 10, height 20, split at z = 10 and drafted 3 degrees
    /// on each side away from that circle becomes two joined frusta. With
    /// r = 10 - 10 tan(3 degrees), each has volume pi*10*(100 + 10r + r²)/3.
    /// The coaxial positive-nappe surfaces meet only at their neutral circle.
    #[test]
    fn a_cylinder_parted_both_ways_is_closed_with_the_two_frusta_volume() {
        all_fixes();
        let c = make_cylinder([0.0; 3], 10.0, 20.0).unwrap();
        let o = order(&c);
        let parting = crate::split::Tool {
            surface: solidcore_record::brep::Surface::Plane {
                origin: [0.0, 0.0, 10.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            },
            domain: [[-20.0, 20.0], [-20.0, 20.0]],
        };
        let (parted, _) = crate::split::split_faces(&c, &o, &[curved(&c, &o)], &[parting])
            .unwrap()
            .unwrap();
        // the record index (through `o`) of the one face of `b` through `p`
        let through = |b: &crate::Body, o: &Order, p: [f64; 3]| {
            let hits: Vec<usize> = (0..o.faces.len())
                .filter(|&i| b.face_distances(o, i, &[p]).unwrap()[0].is_some_and(|d| d < 1e-9))
                .collect();
            assert_eq!(hits.len(), 1, "faces through {p:?}: {hits:?}");
            hits[0]
        };
        let small = 10.0 - 10.0 * TAN_THREE;
        let frustum = PI * 10.0 * (100.0 + 10.0 * small + small * small) / 3.0;
        for (step, (at, pull)) in [([10.0, 0.0, 15.0], Z), ([10.0, 0.0, 5.0], [0.0, 0.0, -1.0])]
            .into_iter()
            .enumerate()
        {
            let o = order(&parted);
            let half = through(&parted, &o, at);
            draft(&parted, &o, &[half], [0.0, 0.0, 10.0], Z, pull, THREE).unwrap();
            let expected = if step == 0 {
                1000.0 * PI + frustum
            } else {
                2.0 * frustum
            };
            let measured = volume(&parted);
            assert!(
                rel(measured, expected) < 1e-9,
                "step {step}: {measured} vs {expected}"
            );
            assert!(parted.is_manifold_solid().unwrap());
            parted.check_solid().unwrap();
            let report = parted.check(crate::CheckLevel::All).unwrap();
            assert_eq!(report.failures, 0, "step {step}: {}", report.text);
            println!(
                "parted cylinder step={step}: measured={measured:.12}, expected={expected:.12}"
            );
        }
    }

    /// #18374: a cylinder r 1.5, z 0 to 4, with a 0.36 cube joined on its top 0.12 in from the
    /// rim (x 1.02 to 1.38), its side drafted 3 degrees about its base. The side's cone is
    /// r 1.5 - 4 tan 3 degrees (about 1.29) at the top, so the cube now overhangs it; the cube
    /// stays as it was, its underside outside the cone uncovered. The frustum plus the cube,
    /// in nine faces: the cone, the base, the top, the cube's underside past the cone, and
    /// the cube's five.
    #[test]
    fn a_cylinder_with_a_boss_near_its_rim_drafts_under_the_boss() {
        all_fixes();
        let cylinder = make_cylinder([0.0; 3], 1.5, 4.0).unwrap();
        let cube = make_box([1.02, -0.18, 4.0], [0.36, 0.36, 0.36]).unwrap();
        let b = crate::boolean::boolean(cylinder, cube, crate::BooleanOp::Union)
            .unwrap()
            .unwrap();
        let o = order(&b);
        draft(&b, &o, &[curved(&b, &o)], [0.0; 3], Z, Z, THREE).unwrap();
        let (big, h) = (1.5, 4.0);
        let small = big - h * TAN_THREE;
        let v = PI * h * (big * big + big * small + small * small) / 3.0 + 0.36 * 0.36 * 0.36;
        let got = volume(&b);
        assert!(rel(got, v) < 1e-9, "{got} {v}");
        assert_eq!(b.counts().unwrap()[0], 9);
        assert!(b.is_manifold_solid().unwrap());
        b.check_solid().unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "{}", report.text);
    }

    /// #20767 (stress corpus S8188): a cone r 2 at z = 0 to r 1 at z = 3 shelled 0.12 in to a
    /// closed cavity, its side drafted 3 degrees about its base, leaning in going up. The side's
    /// wall is 0.12 / cos(atan(1/3)) = 0.04 sqrt(10) across, horizontally, so the cavity's rim at
    /// z = 2.88 has radius 2 - 0.04 sqrt(10) - 0.96. Drafted by theta the side is
    /// r = 2 - z tan(atan(1/3) + theta), which reaches the rim at
    /// theta = atan((2 - rim) / 2.88) - atan(1/3), 2.234 degrees: 3 degrees would carry the side
    /// through the cavity. Refused naming that angle, the body as it came in (the cone's
    /// 7 pi less the cavity, pi 0.92 (R^2 + R rim + rim^2), R = rim + 0.92 the radius at
    /// z = 0.12). With a 0.5 wall the rim is reached only at 10.118 degrees, and 3 degrees builds
    /// the drafted cone, top radius 2 - 3 tan(atan(1/3) + 3 degrees), less the cavity.
    #[test]
    fn a_shelled_cones_side_drafted_into_its_cavity_is_refused_naming_the_largest_angle() {
        // atan((2 - rim) / 2.88) - atan(1/3), rim = 2 - 0.04 sqrt(10) - 0.96
        const LARGEST: f64 = 0.038_994_633_885_685_75;
        // 7 pi less the cavity, rim and R = rim + 0.92 as above
        const SHELLED: f64 = 5.021_880_973_325_445;
        // the 0.5 wall's: the drafted cone less the cavity z 0.5 to 2.5,
        // rim 2 - sqrt(10) / 6 - 2.5 / 3
        const DRAFTED: f64 = 13.675_559_992_581_547;
        all_fixes();
        for (wall, drafts) in [(0.12, false), (0.5, true)] {
            let c = make_cone([0.0; 3], 2.0, 1.0, 3.0).unwrap();
            crate::offset::shell(&c, &order(&c), &[], -wall).unwrap();
            let o = order(&c);
            let side = face(&c, &o, |bx| bx.max().x > 1.99 && bx.max().z > 2.99);
            let got = draft(&c, &o, &[side], [0.0; 3], Z, Z, THREE);
            if drafts {
                assert_eq!(got, Ok(()));
                assert!(rel(volume(&c), DRAFTED) < 1e-9, "{}", volume(&c));
                assert!(c.is_manifold_solid().unwrap());
                continue;
            }
            match got {
                Err(Error::OffsetRefused {
                    face,
                    why: crate::OffsetRefusal::DraftClosesWall { largest },
                }) => {
                    assert_eq!(face, Some(side));
                    assert!((largest - LARGEST).abs() < 1e-12, "{largest}");
                }
                other => panic!("{other:?}"),
            }
            assert!(rel(volume(&c), SHELLED) < 1e-9, "{}", volume(&c));
            // the cavity's side drafted with it: the cavity's undrafted vertices no longer say
            // where the walls meet, so no bound is named (the local operation's check judges it)
            let cavity = face(&c, &o, |bx| {
                bx.min().z > 0.11 && bx.max().z - bx.min().z > 1.0
            });
            let k = solidcore::Kernel::default();
            let brep = c.lock();
            let check = |faces: &[usize]| {
                let faces: Vec<_> = faces.iter().map(|&i| o.faces[i]).collect();
                crate::offset::refuse_draft_closing_wall(&k, &brep, &faces, [0.0; 3], Z, Z, THREE)
            };
            assert!(check(&[side]).is_err());
            assert!(check(&[side, cavity]).is_ok());
        }
    }

    /// A finite radial removal leaves an overhang above the fixed top plane and
    /// removes a shallow blind hole wholly outside the new conical wall.
    #[test]
    fn a_drafted_cylinder_keeps_its_boss_and_loses_a_hole_beyond_the_new_wall() {
        all_fixes();
        let cylinder = make_cylinder([0.0; 3], 1.5, 4.0).unwrap();
        let boss = make_box([1.02, -0.36, 4.0], [0.36, 0.72, 0.36]).unwrap();
        let b = crate::boolean::boolean(cylinder, boss, crate::BooleanOp::Union)
            .unwrap()
            .unwrap();
        let hole = make_cylinder([-1.4, 0.0, 3.8], 0.05, 0.2).unwrap();
        let b = crate::boolean::boolean(b, hole, crate::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let o = order(&b);
        let outer = face(&b, &o, |bx| {
            bx.min().x < 0.0 && bx.max().x > 0.0 && bx.max().z > bx.min().z
        });
        draft(&b, &o, &[outer], [0.0; 3], Z, Z, THREE).unwrap();
        // The hole's nearest radius is 1.35, beyond r(3.8) < 1.301. None
        // survives. Above z = 4 the entire boss stays, including its overhang.
        let small = 1.5 - 4.0 * TAN_THREE;
        let expected =
            PI * 4.0 * (1.5 * 1.5 + 1.5 * small + small * small) / 3.0 + 0.36 * 0.72 * 0.36;
        assert!(rel(volume(&b), expected) < 1e-9);
        assert_eq!(b.counts().unwrap()[0], 9);
        b.check_solid().unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "{}", report.text);
    }

    /// S1767 of the public stress corpus: two touching pads, the second 0.00002
    /// taller, stay above the cylinder's top while its side drafts under their feet.
    #[test]
    #[ignore = "S1767: the joined input fails draft's solid precondition (#18805)"]
    fn a_cylinder_with_touching_stepped_bosses_drafts_under_both() {
        all_fixes();
        let cylinder = make_cylinder([0.0; 3], 1.5, 4.0).unwrap();
        let first = make_box([1.02, -0.18, 4.0], [0.36, 0.36, 0.36]).unwrap();
        let second = make_box([1.02, 0.18, 4.0], [0.36, 0.36, 0.36002]).unwrap();
        let b = crate::boolean::boolean(cylinder, first, crate::BooleanOp::Union)
            .unwrap()
            .unwrap();
        let b = crate::boolean::boolean(b, second, crate::BooleanOp::Union)
            .unwrap()
            .unwrap();
        let o = order(&b);
        draft(&b, &o, &[curved(&b, &o)], [0.0; 3], Z, Z, THREE).unwrap();
        // The entire pads stay unchanged above z = 4. Their shared face has no volume.
        let small = 1.5 - 4.0 * TAN_THREE;
        let expected = PI * 4.0 * (1.5 * 1.5 + 1.5 * small + small * small) / 3.0
            + 0.36 * 0.36 * (0.36 + 0.36002);
        assert!(rel(volume(&b), expected) < 1e-9);
        assert!(b.is_manifold_solid().unwrap());
        b.check_solid().unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "{}", report.text);
    }

    /// S8075 of the public stress corpus: drafting the outer wall opens the three
    /// blind holes through the side. Their walls must be trimmed against the cone.
    #[test]
    fn a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls() {
        all_fixes();
        let mut b = make_cylinder([0.0; 3], 600.0, 2400.0).unwrap();
        for y in [-180.0, 0.0, 180.0] {
            let hole = make_cylinder([-480.0, y, 1920.0], 60.0, 480.0).unwrap();
            b = crate::boolean::boolean(b, hole, crate::BooleanOp::Subtract)
                .unwrap()
                .unwrap();
        }
        let o = order(&b);
        let outer = face(&b, &o, |bx| {
            bx.min().x < 0.0 && bx.max().x > 0.0 && bx.max().z > bx.min().z
        });
        draft(&b, &o, &[outer], [0.0; 3], Z, Z, THREE).unwrap();
        // r(z) = 600 - z tan(3 degrees). Below the blind floors (-480, 0)
        // is material; above them each hole remains void on its inward side.
        // At z = 2000 the cone radius is between 495 and 496, so (500, 0)
        // is outside although it was inside the original cylinder.
        for (point, wanted) in [
            ([-480.0, 0.0, 1900.0], crate::PointClass::Inside),
            ([500.0, 0.0, 2000.0], crate::PointClass::Outside),
            ([-410.0, 0.0, 2300.0], crate::PointClass::Inside),
            ([-430.0, -180.0, 2300.0], crate::PointClass::Outside),
            ([-430.0, 0.0, 2300.0], crate::PointClass::Outside),
            ([-430.0, 180.0, 2300.0], crate::PointClass::Outside),
        ] {
            assert_eq!(b.classify_point(point).unwrap(), wanted, "{point:?}");
        }
        assert!(b.is_manifold_solid().unwrap());
        b.check_solid().unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "{}", report.text);
    }

    /// A frustum r 100 at z = 0 to r 40 at z = 200 (its side leaning atan 0.3, 16.70 degrees,
    /// inward) with six blind holes r 5, from z = 150 up through the top, their centres on the
    /// circle r 32 (each reaching out to r 37). Its side drafted to lean 18 degrees, about its
    /// base, is r(z) = 100 - z tan 18 degrees, which passes r 37 at z0 = 193.89 and is r 35.02
    /// at the top: every hole breaks out through the drafted side near the top, its wall and
    /// the cone's passing through each other between the two points where the hole's rim
    /// crosses the top's new rim. The body is the frustum less the holes: pi int r^2 less
    /// 6 int lens(r(z), 5, 32) over z 150 to 200, the lens the discs' closed-form overlap,
    /// 25 pi below z0. The lens integral above z0, after z = z0 + s^2 (which makes it
    /// analytic), by Gauss-Legendre: 20, 40 and 80 nodes agree to every digit printed,
    /// 3061182.2920648409. (At this size the drafted cone's stored top height, recovered from
    /// its net, is 200 plus a rounding, though its top row lies in the top's plane.)
    #[test]
    fn a_cones_side_drafted_through_blind_holes_near_its_top_breaks_them_out() {
        /// 18 degrees less atan 0.3, radians: the turn that leaves the side at 18 degrees.
        const TURN: f64 = 0.022_702_470_881_112_213;
        /// The drafted frustum less the six holes (the quadrature above).
        const DRAFTED: f64 = 3_061_182.292_064_841;
        /// sqrt 3, the double nearest it (ops computes no geometry, its tests included).
        const SQRT_3: f64 = 1.732_050_807_568_877_2;
        all_fixes();
        let mut b = make_cone([0.0; 3], 100.0, 40.0, 200.0).unwrap();
        let s = SQRT_3;
        for (x, y) in [
            (32.0, 0.0),
            (16.0, 16.0 * s),
            (-16.0, 16.0 * s),
            (-32.0, 0.0),
            (-16.0, -16.0 * s),
            (16.0, -16.0 * s),
        ] {
            let hole = make_cylinder([x, y, 150.0], 5.0, 51.0).unwrap();
            b = crate::boolean::boolean(b, hole, crate::BooleanOp::Subtract)
                .unwrap()
                .unwrap();
        }
        let o = order(&b);
        let side = face(&b, &o, |bx| {
            bx.min().x < 0.0 && bx.max().x > 0.0 && bx.max().z > bx.min().z
        });
        draft(&b, &o, &[side], [0.0; 3], Z, Z, TURN).unwrap();
        let got = volume(&b);
        assert!(rel(got, DRAFTED) < 1e-9, "{got} vs {DRAFTED}");
        // r(190) = 38.27 and the old side's r(190) = 43: (40 cos 30, 40 sin 30, 190), between
        // two holes, is cut away; r 30 there stays; the hole's centre line and its outer reach
        // past the new side near the top are void.
        for (point, wanted) in [
            ([20.0 * s, 20.0, 190.0], crate::PointClass::Outside),
            ([15.0 * s, 15.0, 195.0], crate::PointClass::Inside),
            ([32.0, 0.0, 170.0], crate::PointClass::Outside),
            ([36.0, 0.0, 199.5], crate::PointClass::Outside),
            ([30.0, 0.0, 100.0], crate::PointClass::Inside),
        ] {
            assert_eq!(b.classify_point(point).unwrap(), wanted, "{point:?}");
        }
        assert!(b.is_manifold_solid().unwrap());
        b.check_solid().unwrap();
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert_eq!(report.failures, 0, "{}", report.text);
    }
}

/// Kernel diagnostics cross the application seam here; the app receives only ops
/// data and need not name the kernel's reason classes or construct kernel errors.
#[test]
fn offset_failures_keep_the_kernel_reason_and_class() {
    use crate::OffsetRefusal;
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::Failure;
    use solidcore::{EntityRef, FaceId};

    for why in [
        OffsetFailure::Failed("the offset edge has no intersection"),
        OffsetFailure::Missing("the face's surface"),
        OffsetFailure::InvalidValue("the offset distance is zero"),
        OffsetFailure::Inconsistent("the edge's mate is absent"),
    ] {
        let error = Failure::new("offset::offset_executive::crossing_loops", why)
            .at(EntityRef::Face(FaceId::from_index(2)))
            .into_error();
        let diagnostic = super::diagnostic(&error);
        assert_eq!(diagnostic.detail, error.to_string());
        assert_eq!(
            diagnostic.refusal,
            Some(Error::OffsetRefused {
                face: None,
                why: OffsetRefusal::Phrase {
                    class: why.kind(),
                    defect: why.is_defect(),
                    phrase: error.to_string()
                },
            })
        );
    }
}

#[test]
fn typed_offset_refusals_keep_their_nested_kernel_cause() {
    use crate::OffsetRefusal;
    use solidcore::error::intersect::IntersectFailure;
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::Failure;

    let cause = Failure::new(
        "intersect::clip",
        IntersectFailure::Failed("the exact trim does not cross the face boundary"),
    )
    .into_error();
    let error = Failure::new("offset::trim", OffsetFailure::ExactCurveUncut)
        .caused_by(cause)
        .into_error();
    let diagnostic = super::diagnostic(&error);
    assert_eq!(
        diagnostic.refusal,
        Some(Error::OffsetRefused {
            face: None,
            why: OffsetRefusal::ExactCurveUncut
        })
    );
    assert_eq!(diagnostic.detail, error.to_string());
    assert!(
        diagnostic
            .detail
            .contains("intersect::clip: the exact trim does not cross the face boundary"),
        "{}",
        diagnostic.detail
    );

    let error = Failure::new(
        "offset::face",
        OffsetFailure::Folds {
            face: solidcore::FaceId::from_index(2),
            distance: 2.0,
            largest: 1.0,
        },
    )
    .caused_by(error)
    .into_error();
    let diagnostic = super::diagnostic(&error);
    assert_eq!(
        diagnostic.refusal,
        Some(Error::WallFolds { largest: Some(1.0) })
    );
    assert_eq!(diagnostic.detail, error.to_string());
}

/// Intersections and Booleans inside shelling also keep their named cause,
/// including the failure beneath an offset stage.
#[test]
fn offset_substages_keep_their_failure_reason() {
    use solidcore::error::intersect::IntersectFailure;
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::{Failure, Reason};

    let cause = Failure::new(
        "intersect::surface_surface",
        IntersectFailure::Failed("the two face trims do not cross"),
    )
    .into_error();
    let offset = Failure::new("offset::offset_executive", OffsetFailure::StageFailed)
        .caused_by(cause.clone())
        .into_error();
    let boolean = Failure::new(
        "boolean::merge::p3",
        Reason::Failed("a Boolean of the tree failed"),
    )
    .caused_by(cause.clone())
    .into_error();
    for error in [cause, offset, boolean] {
        let diagnostic = super::diagnostic(&error);
        assert_eq!(diagnostic.detail, error.to_string());
        assert!(
            diagnostic
                .detail
                .contains("the two face trims do not cross"),
            "{}",
            diagnostic.detail
        );
    }
}

/// The index of the edge of `body` through `p`, in its edge list.
fn edge_at(body: &Body, p: [f64; 3]) -> usize {
    let k = Kernel::default();
    let b = body.lock();
    let p = Point3::new(p[0], p[1], p[2]);
    b.edge_list()
        .iter()
        .position(|&e| edge_closest_point(&k, &b, e, p).is_ok_and(|c| c.closest.distance < 1e-9))
        .unwrap()
}

/// A shell over a 4 mm G2 corner names its largest wall, 0.9674 mm, rather than "not proved
/// regular" (#17246).
#[test]
fn shell_of_g2_corner_names_largest_wall() {
    all_fixes();
    let b = make_box([-7.5, -7.5, 0.0], [36.5, 38.5, 10.0]).unwrap();
    // the three edges meeting at the corner (29, 31, 10)
    let edges = [
        edge_at(&b, [10.75, 31.0, 10.0]),
        edge_at(&b, [29.0, 11.75, 10.0]),
        edge_at(&b, [29.0, 31.0, 5.0]),
    ];
    blend(&b, Blend::Curvature, &edges, &[4.0], &[], Corners::Filled).unwrap();
    let (_, order) = b.to_record().unwrap();
    let counts = b.counts().unwrap();
    let mut top: Option<(usize, f64)> = None;
    for i in 0..counts[0] {
        let c = b.face_centroid(&order, i, 1e-8).unwrap();
        if c.centroid[2] >= 9.999 && top.is_none_or(|(_, a)| c.area > a) {
            top = Some((i, c.area));
        }
    }
    let (top, _) = top.expect("a top face at z = 10");
    match shell(&b, &order, &[top], -1.0) {
        Err(Error::WallFolds { largest: Some(l) }) => {
            assert!(
                (l - 0.9674).abs() < 1e-3,
                "largest wall {l}, expected 0.9674"
            );
        }
        other => panic!("expected WallFolds naming the largest wall, got {other:?}"),
    }
}

/// A 40 x 30 x 16 block (z -3 to 13) with a countersunk hole from below about the line
/// x = y = 0 (#19904): a hole r 2 through it, a well r 4 from its underside up to z = 1.5,
/// and a cone from r `base` at z = 1.5 to r 2 at z = 3.5 between them. With `base` 3.99 a
/// flat ring 0.01 wide (0.25 mm²) is left at z = 1.5 between the cone and the well; with 4
/// the cone meets the well.
fn countersunk_block(base: f64) -> Body {
    let cut = |a: Body, tool: Body| {
        crate::boolean::boolean(a, tool, crate::BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    };
    let b = make_box([-20.0, -15.0, -3.0], [40.0, 30.0, 16.0]).unwrap();
    let b = cut(b, make_cylinder([0.0, 0.0, -5.0], 2.0, 20.0).unwrap());
    let b = cut(b, make_cylinder([0.0, 0.0, -3.0], 4.0, 4.5).unwrap());
    cut(
        b,
        crate::make_cone([0.0, 0.0, 1.5], base, 2.0, 2.0).unwrap(),
    )
}

/// The countersunk block's volume: the block less the hole, the well beside the hole and
/// the frustum beside the hole, `19200 - 16 pi 4 - 4.5 pi (16 - 4) - (2 pi (R² + 2R + 4) / 3 - 8 pi)`.
fn countersunk_volume(base: f64) -> f64 {
    19200.0 - 64.0 * PI - 54.0 * PI - (2.0 * PI * (base * base + 2.0 * base + 4.0) / 3.0 - 8.0 * PI)
}

/// The countersunk block's cone, by its point at z = 2.5 (off the seam at y = 0).
fn countersink_cone(b: &Body, o: &Order, base: f64) -> usize {
    through(b, o, [0.0, base - (base - 2.0) / 2.0, 2.5])
}

/// Without a ring the countersink's cone (45°) moved 2 into the material deepens the seat:
/// the cone moves 2 / sin 45° = 2√2 along its axis, the well's wall runs up to meet it and
/// the hole's wall is trimmed, so the annulus r 2 to 4 loses a slab 2√2 high,
/// V - 12 pi 2√2.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_countersinks_cone_moves_into_the_material_over_the_well() {
    all_fixes();
    let b = countersunk_block(4.0);
    let o = order(&b);
    let cone = countersink_cone(&b, &o, 4.0);
    assert!(rel(volume(&b), countersunk_volume(4.0)) < 1e-12);
    offset_faces(&b, &o, &[cone], -2.0).unwrap();
    let expected = countersunk_volume(4.0) - 12.0 * PI * 2.0 * 2.0_f64.sqrt();
    assert!(
        rel(volume(&b), expected) < 1e-9,
        "{} vs {expected}",
        volume(&b)
    );
    b.check_solid().unwrap();
}

/// Without a ring the countersink's cone (45°) moved δ off the material, toward the axis,
/// lowers the seat: in the meridian plane the cone `r + z = 5.5` becomes
/// `r + z = 5.5 - δ√2` and meets the hole r 2 at z = 3.5 - δ√2 and the well r 4 at
/// z = 1.5 - δ√2, both δ√2 lower, so the cone between them keeps its length 2√2 and closes at
/// no δ: the hole's wall runs down and the well's is trimmed until the cone reaches the
/// well's foot, z = -3, at δ = 4.5 / √2 = 3.18. The annulus r 2 to 4 gains a slab δ√2 high,
/// V + 12 pi δ√2. Past δ = 2 sin 45° = √2 the moved cone's apex, z = 5.5 - δ√2, is below
/// the hole's old edge at z = 3.5, and the meeting there was sought from the edge on the
/// cone's other nappe: +1.5 and +2 were refused for a reach with no certified bound (#20935).
/// Past δ = 2 / cos 45° = 2√2 the moved cone's apex is past the image the cone's own
/// parameters give the hole's circle, r = 2 - δ cos 45°: the face lost that edge and was
/// refused as unsound, though the well's wall is still a band 4.5 - δ√2 high, 0.26 at 3 and
/// 0.0027 at 3.18 (#21188).
/// Moved 3.5, past the foot, the well's wall between the cone and the block's underside
/// closes at 4.5 / √2 and is refused, naming it.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_countersinks_cone_moves_off_the_material_past_its_apex_over_the_hole() {
    all_fixes();
    for delta in [1.0, 1.4, 1.5, 2.0] {
        let b = countersunk_block(4.0);
        let o = order(&b);
        let cone = countersink_cone(&b, &o, 4.0);
        offset_faces(&b, &o, &[cone], delta).unwrap();
        let expected = countersunk_volume(4.0) + 12.0 * PI * delta * 2.0_f64.sqrt();
        assert!(
            rel(volume(&b), expected) < 1e-9,
            "{delta}: {} vs {expected}",
            volume(&b)
        );
        b.check_solid().unwrap();
    }
    // past 2√2, up to just short of the foot at 4.5 / √2 = 3.182
    for delta in [2.9, 3.0, 3.1, 3.18] {
        let b = countersunk_block(4.0);
        let o = order(&b);
        let cone = countersink_cone(&b, &o, 4.0);
        offset_faces(&b, &o, &[cone], delta).unwrap();
        let expected = countersunk_volume(4.0) + 12.0 * PI * delta * 2.0_f64.sqrt();
        assert!(
            rel(volume(&b), expected) < 1e-9,
            "{delta}: {} vs {expected}",
            volume(&b)
        );
        b.check_solid().unwrap();
    }
    // past the well's foot the well's wall, between the cone and the block's underside,
    // closes: refused before anything changes, naming the well and 4.5 / √2
    let b = countersunk_block(4.0);
    let o = order(&b);
    let cone = countersink_cone(&b, &o, 4.0);
    let well = through(&b, &o, [0.0, 4.0, 0.0]);
    match offset_faces(&b, &o, &[cone], 3.5) {
        Err(Error::OffsetRefused {
            face: named,
            why: crate::OffsetRefusal::FaceCloses { largest },
        }) => {
            assert_eq!(named, Some(well));
            let room = 4.5 / 2.0_f64.sqrt();
            assert!((largest - room).abs() < 1e-9, "{largest} vs {room}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), countersunk_volume(4.0)) < 1e-12);
}

/// With the cone's base r 3.99 a ring 0.01 wide lies at z = 1.5 between the cone (semi-angle
/// θ, tan θ = 1.99 / 2) and the well r 4 (#19904). Moved into the material by δ, the cone meets
/// the ring's plane at r = 3.99 + δ / cos θ: the ring closes at δ = 0.01 cos θ, and moved 2
/// the cone meets that plane at r 6.81, past the well, where keeping the ring would turn it
/// inside out. The move is refused before anything changes, naming the ring and 0.01 cos θ
/// (it was refused late, as a body that fails its check). Moved the other way the cone meets
/// the hole r 2 at z = 3.5 - δ / sin θ and the ring's plane at r = 3.99 - δ / cos θ: the cone
/// itself closes at δ = 2 sin θ, so +2 is refused naming the cone and 2 sin θ (it was refused
/// for a reach with no certified bound). Moved +4 the same: the cone closes at the same
/// 2 sin θ, at r 2, though by the end of the move its crossing with the ring's plane,
/// r = 3.99 - 4 / cos θ = -1.65, has passed the axis.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_ring_beside_a_countersinks_cone_that_the_move_closes_is_refused_naming_it() {
    all_fixes();
    let side = (1.99_f64 * 1.99 + 4.0).sqrt();
    let (sin, cos) = (1.99 / side, 2.0 / side);
    for (distance, closes, largest) in [
        (-2.0, "ring", 0.01 * cos),
        (2.0, "cone", 2.0 * sin),
        (4.0, "cone", 2.0 * sin),
    ] {
        let b = countersunk_block(3.99);
        let o = order(&b);
        let cone = countersink_cone(&b, &o, 3.99);
        let ring = through(&b, &o, [0.0, 3.995, 1.5]);
        let before = volume(&b);
        assert!(rel(before, countersunk_volume(3.99)) < 1e-12);
        let face = if closes == "ring" { ring } else { cone };
        match offset_faces(&b, &o, &[cone], distance) {
            Err(Error::OffsetRefused {
                face: named,
                why: crate::OffsetRefusal::FaceCloses { largest: room },
            }) => {
                assert_eq!(named, Some(face), "{closes}");
                assert!(
                    (room - largest).abs() < 1e-9,
                    "{closes}: {room} vs {largest}"
                );
            }
            other => panic!("{closes}: {other:?}"),
        }
        assert!(rel(volume(&b), before) < 1e-12);
    }
}

/// The ringed countersink's cone moved 0.005 into the material, within the 0.01 cos θ the
/// ring allows, moves: the ring narrows to 0.01 - 0.005 / cos θ and the hole's wall is
/// trimmed. The slab between the cone's two places, δ / sin θ high over r 2 to 3.99, and the
/// ring's part under the new cone, from r 3.99 to ρ = 3.99 + δ / cos θ where the cone rises
/// (ρ - r) / tan θ over the ring's plane, go:
/// `2 pi (δ / sin θ (3.99² - 4) / 2 + (ρ (ρ² - 3.99²) / 2 - (ρ³ - 3.99³) / 3) / tan θ)`.
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_ringed_countersinks_cone_moves_within_the_ring() {
    all_fixes();
    let side = (1.99_f64 * 1.99 + 4.0).sqrt();
    let (sin, cos, tan) = (1.99 / side, 2.0 / side, 1.99 / 2.0);
    let b = countersunk_block(3.99);
    let o = order(&b);
    let cone = countersink_cone(&b, &o, 3.99);
    let delta = 0.005;
    offset_faces(&b, &o, &[cone], -delta).unwrap();
    let (r, rho) = (3.99_f64, 3.99 + delta / cos);
    let removed = 2.0
        * PI
        * (delta / sin * (r * r - 4.0) / 2.0
            + (rho * (rho * rho - r * r) / 2.0 - (rho.powi(3) - r.powi(3)) / 3.0) / tan);
    let expected = countersunk_volume(3.99) - removed;
    assert!(
        rel(volume(&b), expected) < 1e-9,
        "{} vs {expected}",
        volume(&b)
    );
    b.check_solid().unwrap();
}
