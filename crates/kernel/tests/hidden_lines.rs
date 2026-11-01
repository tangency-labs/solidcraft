//! Hidden lines of bodies the kernel makes, through the production `prepare`, against closed forms.

use std::f64::consts::{FRAC_PI_3, PI};

use solidcraft_geom::hlr::{Line, Options, Placement, Source, View, Visibility, hidden_lines, prepare};
use solidcraft_geom::{Seg2, Vec2, Vec3, brep};
use solidcraft_kernel::*;

const TOLERANCE: f64 = 1e-3;

/// Points along a drawn piece, a sixteenth of it apart.
fn points(s: &Seg2) -> Vec<Vec2> {
    (0..=16).map(|i| s.point_at(f64::from(i) / 16.0)).collect()
}

/// The lines of `body`, drawn looking along `d`, with the record they number into.
fn draw(body: &Body, d: Vec3, up: Vec3) -> (Vec<Line>, brep::Brep) {
    let record = brep_record(body).unwrap();
    let o = Options::default();
    let def = prepare(&record, &o).unwrap();
    let placed = Placement { body: 0, rotation: [Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)], translation: Vec3::ZERO };
    let view = View::new(d, up).unwrap();
    let drawing = hidden_lines(std::slice::from_ref(&def), &[placed], &view, &o, &|| false).unwrap();
    (drawing.lines, record)
}

/// Whether a line is an edge whose record is a circle (`want`), or another edge.
fn is_circle(l: &Line, record: &brep::Brep, want: bool) -> bool {
    matches!(l.source, Source::Edge { edge, .. } if matches!(record.edges.get(edge).map(|e| &e.curve), Some(brep::Curve::Circle { .. })) == want)
}

/// Total drawn length of `lines` of a visibility.
fn total<'a>(lines: impl Iterator<Item = &'a Line>, v: Visibility) -> f64 {
    lines.flat_map(|l| &l.pieces).filter(|p| p.visibility == v).flat_map(|p| &p.geometry).map(Seg2::length).sum()
}

/// Total length of what lies under a visible line: seen straight along a wall, a far edge is at
/// best uncertain, and is not drawn because the near edge is.
fn under<'a>(lines: impl Iterator<Item = &'a Line>) -> f64 {
    lines.flat_map(|l| &l.pieces).filter(|p| p.covered && p.visibility != Visibility::Visible).flat_map(|p| &p.geometry).map(Seg2::length).sum()
}

fn uncovered_uncertain(lines: &[Line]) -> usize {
    lines.iter().flat_map(|l| &l.pieces).filter(|p| p.visibility == Visibility::Uncertain && !p.covered).count()
}

fn near(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() <= TOLERANCE, "{what}: {got} against {want}");
}

/// A cylinder of radius 10 cut by the plane x = 5: rims from -60 to 60 degrees. Seen along z each
/// rim is one arc of a third of a turn on the side x >= 5, the near one visible, the far one
/// behind it; a rim on the wrong angles would leave that side.
#[test]
fn a_cut_cylinders_rims_are_drawn_on_their_own_arc() {
    let round = cylinder(Vec3::ZERO, Vec3::new(0., 0., 1.), 10., 20.).unwrap();
    let keep = box_solid(Vec3::new(5., -20., -5.), Vec3::new(25., 20., 25.)).unwrap();
    let body = boolean(&round, &keep, BoolOp::Intersect).unwrap().unwrap();
    let (lines, record) = draw(&body, Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.));
    let rim = 10. * 2. * FRAC_PI_3;
    let rims = || lines.iter().filter(|l| is_circle(l, &record, true));
    near(total(rims(), Visibility::Visible), rim, "near rim");
    near(under(rims()), rim, "far rim");
    assert_eq!(uncovered_uncertain(&lines), 0, "{lines:?}");
    let drawn: Vec<Vec2> = rims().flat_map(|l| &l.pieces).flat_map(|p| &p.geometry).flat_map(points).collect();
    assert!(drawn.len() > 8, "{lines:?}");
    // Whichever way the view's x runs, the rims keep to one side of the axis, at the radius.
    let side = drawn.iter().map(|p| p.x).fold(0., |a: f64, x| if x.abs() > a.abs() { x } else { a }).signum();
    for p in &drawn {
        assert!((p.x.hypot(p.y) - 10.).abs() <= TOLERANCE && p.x * side >= 5. - TOLERANCE, "{p:?}");
    }
    // The chord closes them: 2 * 10 * sin 60 on each end of the flat.
    let flats = || lines.iter().filter(|l| is_circle(l, &record, false));
    let chord = 20. * FRAC_PI_3.sin();
    near(total(flats(), Visibility::Visible), chord, "near chord");
    near(under(flats()), chord, "far chord");
    // From the flat side, along x, the rims are level lines as wide as the chord.
    let (lines, record) = draw(&body, Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.));
    let wide: Vec<Vec2> = lines.iter().filter(|l| is_circle(l, &record, true)).flat_map(|l| &l.pieces).flat_map(|p| &p.geometry).flat_map(points).collect();
    let reach = wide.iter().map(|p| p.x.abs()).fold(0., f64::max);
    near(reach, chord / 2., "rims from the flat side");
}

/// A hole of radius 5 through a 40 by 40 by 10 plate. Along z its near rim is a whole circle in
/// sight, the far one under it; from the side, its outline is two hidden lines the
/// plate's thickness long and 5 either side of the axis, and nothing of it shows.
#[test]
fn a_holes_rims_and_outline_are_drawn_where_they_are() {
    let plate = box_solid(Vec3::new(-20., -20., 0.), Vec3::new(20., 20., 10.)).unwrap();
    let bore = cylinder(Vec3::new(0., 0., -5.), Vec3::new(0., 0., 1.), 5., 20.).unwrap();
    let body = boolean(&plate, &bore, BoolOp::Cut).unwrap().unwrap();
    let (lines, record) = draw(&body, Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.));
    let rims = || lines.iter().filter(|l| is_circle(l, &record, true));
    near(total(rims(), Visibility::Visible), 2. * PI * 5., "near rim");
    near(under(rims()), 2. * PI * 5., "far rim");
    assert_eq!(uncovered_uncertain(&lines), 0, "{lines:?}");
    let (lines, _) = draw(&body, Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.));
    let outline = || lines.iter().filter(|l| matches!(l.source, Source::Silhouette { .. }));
    near(total(outline(), Visibility::Hidden), 2. * 10., "the outline's hidden length");
    near(total(outline(), Visibility::Visible), 0., "the outline in sight");
    for p in outline().flat_map(|l| &l.pieces).flat_map(|p| &p.geometry).flat_map(points) {
        assert!((p.x.abs() - 5.).abs() <= TOLERANCE, "{p:?}");
    }
}

/// The same hole seen at a slant, so the far rim is really part in sight and part behind the
/// plate: a point of it shows exactly where the near rim, an ellipse on screen, has it inside.
#[test]
fn a_holes_far_rim_shows_only_through_its_near_rim() {
    let plate = box_solid(Vec3::new(-20., -20., 0.), Vec3::new(20., 20., 10.)).unwrap();
    let bore = cylinder(Vec3::new(0., 0., -5.), Vec3::new(0., 0., 1.), 5., 20.).unwrap();
    let body = boolean(&plate, &bore, BoolOp::Cut).unwrap().unwrap();
    let (d, up) = (Vec3::new(0., 0.25, -1.), Vec3::new(0., 1., 0.));
    let (lines, record) = draw(&body, d, up);
    let view = View::new(d, up).unwrap();
    let screen = |p: Vec3| view.screen(p);
    let (ax, by) = (screen(Vec3::new(5., 0., 0.)), screen(Vec3::new(0., 5., 0.)));
    // Radius in the rim's own circle, of a screen point against that rim's centre.
    let radial = |p: Vec2, z: f64| {
        let c = screen(Vec3::new(0., 0., z));
        let (x, y) = (p.x - c[0], p.y - c[1]);
        let det = ax[0] * by[1] - ax[1] * by[0];
        ((x * by[1] - y * by[0]) / det).hypot((ax[0] * y - ax[1] * x) / det)
    };
    let (mut near_rim, mut far_shown, mut far_hidden) = (0., 0., 0.);
    for p in lines.iter().filter(|l| is_circle(l, &record, true)).flat_map(|l| &l.pieces).filter(|p| !p.covered) {
        let drawn: Vec<Vec2> = p.geometry.iter().flat_map(points).collect();
        let len: f64 = p.geometry.iter().map(Seg2::length).sum();
        if drawn.iter().all(|q| (radial(*q, 10.) - 1.).abs() <= 1e-3) {
            assert_eq!(p.visibility, Visibility::Visible, "{p:?}");
            near_rim += len;
            continue;
        }
        assert!(drawn.iter().all(|q| (radial(*q, 0.) - 1.).abs() <= 1e-3), "not on a rim: {p:?}");
        for q in &drawn {
            let inside = radial(*q, 10.);
            match p.visibility {
                Visibility::Visible => assert!(inside <= 1. + 1e-3, "{q:?} shows, {inside} out of the near rim"),
                Visibility::Hidden => assert!(inside >= 1. - 1e-3, "{q:?} hidden, {inside} in the near rim"),
                Visibility::Uncertain => panic!("{p:?}"),
            }
        }
        if p.visibility == Visibility::Visible { far_shown += len } else { far_hidden += len }
    }
    let ellipse: f64 = (0..4096)
        .map(|i| {
            let at = |k: f64| {
                let t = k * std::f64::consts::TAU / 4096.;
                Vec2::new(ax[0] * t.cos() + by[0] * t.sin(), ax[1] * t.cos() + by[1] * t.sin())
            };
            (at(f64::from(i) + 1.) - at(f64::from(i))).len()
        })
        .sum();
    assert!((near_rim - ellipse).abs() <= 0.01, "near rim {near_rim} against {ellipse}");
    assert!((far_shown + far_hidden - ellipse).abs() <= 0.01, "far rim {far_shown} + {far_hidden} against {ellipse}");
    assert!(far_shown > 1. && far_hidden > 1., "{far_shown} {far_hidden}");
}
