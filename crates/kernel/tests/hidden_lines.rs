//! Hidden lines of bodies the kernel makes, through the production `prepare`, against closed forms.

use std::f64::consts::{FRAC_PI_3, PI};

use solidcraft_geom::hlr::{Line, Options, Placement, Source, View, Visibility, hidden_lines, prepare};
use solidcraft_geom::{Seg2, Vec2, Vec3, brep};
use solidcraft_kernel::*;

const TOLERANCE: f64 = 1e-3;

/// Points along a drawn piece, a sixteenth of it apart.
fn points(s: &Seg2) -> Vec<Vec2> {
    (0..=16).map(|i| s.at(f64::from(i) / 16.0)).collect()
}

/// The lines of `body`, drawn looking along `d`, with the record they number into.
fn draw(body: &Body, d: Vec3, up: Vec3) -> (Vec<Line>, brep::Brep) {
    let record = native::brep_record(body).unwrap();
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
    near(total(rims(), Visibility::Hidden), rim, "far rim");
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
    near(total(flats(), Visibility::Hidden), chord, "far chord");
    // From the flat side, along x, the rims are level lines as wide as the chord.
    let (lines, record) = draw(&body, Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.));
    let wide: Vec<Vec2> = lines.iter().filter(|l| is_circle(l, &record, true)).flat_map(|l| &l.pieces).flat_map(|p| &p.geometry).flat_map(points).collect();
    let reach = wide.iter().map(|p| p.x.abs()).fold(0., f64::max);
    near(reach, chord / 2., "rims from the flat side");
}

/// A hole of radius 5 through a 40 by 40 by 10 plate. Along z its near rim is a whole circle in
/// sight, the far one behind the plate; from the side, its outline is two hidden lines the
/// plate's thickness long and 5 either side of the axis, and nothing of it shows.
#[test]
fn a_holes_rims_and_outline_are_drawn_where_they_are() {
    let plate = box_solid(Vec3::new(-20., -20., 0.), Vec3::new(20., 20., 10.)).unwrap();
    let bore = cylinder(Vec3::new(0., 0., -5.), Vec3::new(0., 0., 1.), 5., 20.).unwrap();
    let body = boolean(&plate, &bore, BoolOp::Cut).unwrap().unwrap();
    let (lines, record) = draw(&body, Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.));
    let rims = || lines.iter().filter(|l| is_circle(l, &record, true));
    near(total(rims(), Visibility::Visible), 2. * PI * 5., "near rim");
    near(total(rims(), Visibility::Hidden), 2. * PI * 5., "far rim");
    let (lines, _) = draw(&body, Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.));
    let outline = || lines.iter().filter(|l| matches!(l.source, Source::Silhouette { .. }));
    near(total(outline(), Visibility::Hidden), 2. * 10., "the outline's hidden length");
    near(total(outline(), Visibility::Visible), 0., "the outline in sight");
    for p in outline().flat_map(|l| &l.pieces).flat_map(|p| &p.geometry).flat_map(points) {
        assert!((p.x.abs() - 5.).abs() <= TOLERANCE, "{p:?}");
    }
}
