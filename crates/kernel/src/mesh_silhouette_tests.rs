//! Silhouettes are checked against shapes whose cross-sections are known by construction: a solid
//! is built, frozen to triangles, and read, and every section has to say what the solid measured
//! at that height.

use solidcore_record::Vec3;

use super::*;
use crate::{Body, BoolOp, MeshTolerance, mesh_body};

fn frozen(body: &Body) -> Body {
    let mesh = body.mesh(&MeshTolerance::chord(0.02)).unwrap();
    mesh_body(&mesh.positions, &mesh.triangles).unwrap()
}

fn read(body: &Body, o: &SilhouetteOptions) -> Silhouettes {
    let found = silhouettes(&frozen(body), o).unwrap();
    found.along.into_iter().next().unwrap()
}

/// The radius of a section, measured from its own centre: what a round shape should give back.
fn radii(section: &Section) -> (f64, f64) {
    let n = section.points.len() as f64;
    let cx = section.points.iter().map(|p| p[0]).sum::<f64>() / n;
    let cy = section.points.iter().map(|p| p[1]).sum::<f64>() / n;
    let out = section.points.iter().map(|p| ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt());
    (out.clone().fold(f64::INFINITY, f64::min), out.fold(f64::NEG_INFINITY, f64::max))
}

#[test]
fn a_cylinder_gives_the_same_circle_at_every_height() {
    let found = read(&crate::cylinder(Vec3::ZERO, Vec3::Z, 12., 40.).unwrap(), &SilhouetteOptions::default());
    assert_eq!(found.sections.len(), 16);
    assert_eq!(found.hulled, 0, "a clean cylinder should close at every height");
    for section in &found.sections {
        let (near, far) = radii(section);
        assert!((near - 12.).abs() < 0.1 && (far - 12.).abs() < 0.1, "at {}: {near} to {far}", section.at);
        assert!(section.area > 0., "every profile should turn counter-clockwise");
        assert_eq!(section.points.len(), 60);
    }
    // The cuts stop short of the ends by the margin, and the reach still reports the whole shape.
    assert!(found.reach[0].abs() < 0.05 && (found.reach[1] - 40.).abs() < 0.05, "{:?}", found.reach);
}

#[test]
fn a_sphere_gives_the_circle_its_own_geometry_says_at_every_height() {
    // The case the slab reading refuses, and the one this is for: a shape that changes size all
    // the way along, with no level anywhere to extrude between.
    let found = read(&crate::sphere(Vec3::ZERO, 20.).unwrap(), &SilhouetteOptions { count: 12, margin: 3., ..Default::default() });
    for section in &found.sections {
        let (near, far) = radii(section);
        let want = (400. - section.at * section.at).max(0.).sqrt();
        assert!((near - want).abs() < 0.4 && (far - want).abs() < 0.4, "at {}: wanted {want}, got {near} to {far}", section.at);
    }
    // Widest in the middle, as a sphere is.
    let widest = found.sections.iter().max_by(|a, b| a.area.total_cmp(&b.area)).unwrap();
    assert!(widest.at.abs() < 3., "the widest cut should be near the equator, not at {}", widest.at);
}

#[test]
fn every_profile_starts_at_the_same_bearing_so_the_loft_does_not_twist() {
    // A square prism is the hard case: four corners, and which one is "first" is exactly what
    // flips between sections when the start is picked badly.
    let found = read(&crate::box_solid(Vec3::ZERO, Vec3::new(30., 30., 40.)).unwrap(), &SilhouetteOptions::default());
    let bearing = |section: &Section| {
        let n = section.points.len() as f64;
        let cx = section.points.iter().map(|p| p[0]).sum::<f64>() / n;
        let cy = section.points.iter().map(|p| p[1]).sum::<f64>() / n;
        let first = section.points.first().copied().unwrap_or([0.; 2]);
        (first[1] - cy).atan2(first[0] - cx)
    };
    for section in &found.sections {
        assert!(bearing(section).abs() < 0.2, "a profile starts {} radians off the reference", bearing(section));
    }
}

#[test]
fn a_concavity_survives_where_the_outline_closes() {
    // A bar with a notch cut in one side. Read along Z the notch is in the plane of every cut, so
    // the outline should keep it; a convex hull would fill it in.
    let bar = crate::box_solid(Vec3::ZERO, Vec3::new(40., 20., 30.)).unwrap();
    let notch = crate::box_solid(Vec3::new(15., 12., -5.), Vec3::new(25., 25., 35.)).unwrap();
    let body = crate::boolean(&bar, &notch, BoolOp::Cut).unwrap().unwrap();
    let found = read(&body, &SilhouetteOptions::default());
    assert_eq!(found.hulled, 0, "a clean notch should still chain into one outline");
    let section = found.sections.first().unwrap();
    // 40 x 20 less a 10 x 8 notch.
    assert!((section.area - (800. - 80.)).abs() < 20., "the notch should still be missing: {}", section.area);
}

#[test]
fn asking_for_the_hull_fills_the_concavity_in_and_says_so() {
    let bar = crate::box_solid(Vec3::ZERO, Vec3::new(40., 20., 30.)).unwrap();
    let notch = crate::box_solid(Vec3::new(15., 12., -5.), Vec3::new(25., 25., 35.)).unwrap();
    let body = crate::boolean(&bar, &notch, BoolOp::Cut).unwrap().unwrap();
    let found = read(&body, &SilhouetteOptions { hull: true, ..Default::default() });
    assert_eq!(found.hulled, found.sections.len(), "every section should report that it was hulled");
    let section = found.sections.first().unwrap();
    assert!((section.area - 800.).abs() < 20., "the hull should have filled the notch: {}", section.area);
}

#[test]
fn a_hole_through_the_shape_does_not_turn_the_section_into_a_hull() {
    // The bore leaves two loops at every height, but only one of them is an outer outline, so the
    // silhouette is still the outline and not a hull around both.
    let bar = crate::box_solid(Vec3::ZERO, Vec3::new(40., 40., 30.)).unwrap();
    let bore = crate::cylinder(Vec3::new(20., 20., -5.), Vec3::Z, 8., 40.).unwrap();
    let body = crate::boolean(&bar, &bore, BoolOp::Cut).unwrap().unwrap();
    let found = read(&body, &SilhouetteOptions::default());
    assert_eq!(found.hulled, 0, "a bore is a hole, not a second outline");
    let section = found.sections.first().unwrap();
    assert!((section.area - 1600.).abs() < 30., "the silhouette is the outside, bore and all: {}", section.area);
}

#[test]
fn spacing_sets_the_number_of_cuts() {
    let body = crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 40.).unwrap();
    let found = read(&body, &SilhouetteOptions { spacing: Some(5.), ..Default::default() });
    // 40 long less 1.5 mm off each tip, cut every 5 mm.
    assert_eq!(found.sections.len(), 8, "{:?}", found.sections.iter().map(|s| s.at).collect::<Vec<_>>());
}

#[test]
fn several_axes_are_each_read_on_their_own() {
    let body = frozen(&crate::box_solid(Vec3::ZERO, Vec3::new(60., 40., 20.)).unwrap());
    let found = silhouettes(&body, &SilhouetteOptions { axes: vec![Vec3::X, Vec3::Y, Vec3::Z], count: 6, ..Default::default() }).unwrap();
    assert_eq!(found.along.len(), 3);
    let size = |i: usize| {
        let along = found.along.get(i).unwrap();
        let [u, v] = across(along);
        [u, v, along.reach[1] - along.reach[0]]
    };
    // Read along X the profile is the 40 by 20 end; along Z it is the 60 by 40 face.
    assert!((size(0)[0] - 40.).abs() < 0.2 && (size(0)[1] - 20.).abs() < 0.2, "{:?}", size(0));
    assert!((size(2)[0] - 60.).abs() < 0.2 && (size(2)[1] - 40.).abs() < 0.2, "{:?}", size(2));
}

#[test]
fn a_direction_the_shape_is_flat_along_is_refused() {
    // A sheet of triangles with no thickness: there is nothing to cut across, and the honest
    // answer is to say so rather than to hand back a run of identical profiles.
    let (wide, high) = (6u32, 6u32);
    let points: Vec<Vec3> = (0..=high).flat_map(|j| (0..=wide).map(move |i| Vec3::new(f64::from(i) * 8., f64::from(j) * 6., 0.))).collect();
    let mut triangles = Vec::new();
    for j in 0..high {
        for i in 0..wide {
            let at = j * (wide + 1) + i;
            triangles.push([at, at + 1, at + wide + 2]);
            triangles.push([at, at + wide + 2, at + wide + 1]);
        }
    }
    let sheet = mesh_body(&points, &triangles).unwrap();
    match silhouettes(&sheet, &SilhouetteOptions { axes: vec![Vec3::Z], ..Default::default() }) {
        Err(err) => assert!(err.to_string().contains("flat along that direction"), "{err}"),
        Ok(_) => panic!("a flat sheet read along its own normal should be refused"),
    }
}

#[test]
fn a_thin_plate_is_still_read_rather_than_refused_on_the_margin() {
    // The margin trims the tips, but never so far that nothing is left: a 0.8 mm plate is a
    // perfectly good thing to loft back, and the default margin is wider than the plate.
    let plate = frozen(&crate::box_solid(Vec3::ZERO, Vec3::new(60., 40., 0.8)).unwrap());
    let found = silhouettes(&plate, &SilhouetteOptions::default()).unwrap();
    let along = found.along.first().unwrap();
    assert_eq!(along.sections.len(), 16);
    let [u, v] = across(along);
    assert!((u - 60.).abs() < 0.2 && (v - 40.).abs() < 0.2, "{u} x {v}");
}

#[test]
fn hostile_options_are_refused_rather_than_guessed_at() {
    let body = frozen(&crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 40.).unwrap());
    let refused = |o: SilhouetteOptions| silhouettes(&body, &o).unwrap_err().to_string();
    assert!(refused(SilhouetteOptions { axes: Vec::new(), ..Default::default() }).contains("axes"));
    assert!(refused(SilhouetteOptions { axes: vec![Vec3::ZERO], ..Default::default() }).contains("zero length"));
    assert!(refused(SilhouetteOptions { count: 1, ..Default::default() }).contains("count"));
    assert!(refused(SilhouetteOptions { count: 500, ..Default::default() }).contains("count"));
    assert!(refused(SilhouetteOptions { points: 3, ..Default::default() }).contains("points"));
    assert!(refused(SilhouetteOptions { points: 10_000, ..Default::default() }).contains("points"));
    assert!(refused(SilhouetteOptions { margin: f64::NAN, ..Default::default() }).contains("margin"));
    assert!(refused(SilhouetteOptions { spacing: Some(0.), ..Default::default() }).contains("spacing"));
}

#[test]
fn a_body_that_is_not_a_mesh_is_refused() {
    let err = silhouettes(&crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap(), &SilhouetteOptions::default()).unwrap_err().to_string();
    assert!(err.contains("select a mesh body"), "{err}");
}
