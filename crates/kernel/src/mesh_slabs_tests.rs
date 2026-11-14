//! A slab plan is checked against parts whose answer is known by construction: a solid is built,
//! frozen to triangles, and planned, and the plan has to say what the solid was made from. The
//! tests that matter most add noise to the triangles first, because reading a noisy scan is the
//! whole reason this module exists and a clean mesh would never show whether it works.

use solidcore_record::Vec3;

use super::*;
use crate::{Body, BoolOp, MeshTolerance, mesh_body};

/// The same shape as an imported STL: triangles, with no faces left.
fn frozen(body: &Body) -> Body {
    let mesh = body.mesh(&MeshTolerance::chord(0.01)).unwrap();
    mesh_body(&mesh.positions, &mesh.triangles).unwrap()
}

/// The same thing a scanner would have handed over: every vertex moved off the surface by up to
/// `noise` mm. The jitter is worked out from the vertex's own position, so one mesh always comes
/// back the same and a test that fails fails every time.
fn scanned(body: &Body, noise: f64) -> Body {
    let mesh = body.mesh(&MeshTolerance::chord(0.01)).unwrap();
    let wobble = |seed: f64, salt: f64| {
        let v = (seed * 12.9898 + salt * 78.233).sin() * 43758.5453;
        (v - v.floor()) * 2. - 1.
    };
    let moved: Vec<Vec3> = mesh
        .positions
        .iter()
        .map(|p| {
            let seed = p.x * 1.7 + p.y * 2.3 + p.z * 3.1;
            Vec3::new(p.x + noise * wobble(seed, 1.), p.y + noise * wobble(seed, 2.), p.z + noise * wobble(seed, 3.))
        })
        .collect();
    mesh_body(&moved, &mesh.triangles).unwrap()
}

fn plate(size: Vec3) -> Body {
    crate::box_solid(Vec3::ZERO, size).unwrap()
}

fn drilled(body: &Body, holes: &[(Vec3, f64)]) -> Body {
    let mut out = body.clone();
    for &(at, radius) in holes {
        let tool = crate::cylinder(at - Vec3::Z * 10., Vec3::Z, radius, 200.).unwrap();
        out = crate::boolean(&out, &tool, BoolOp::Cut).unwrap().unwrap();
    }
    out
}

fn options(tolerance: f64, grid: f64) -> SlabOptions {
    SlabOptions { tolerance, grid, ..SlabOptions::default() }
}

/// How far a slab's outline reaches along each of the sketch's own axes, which does not depend on
/// where the datum landed.
fn span(outline: &Loop) -> [f64; 2] {
    let points = flatten(outline);
    [spread(points.iter().map(|p| p[0])), spread(points.iter().map(|p| p[1]))]
}

fn circles(stack: &SlabStack) -> Vec<f64> {
    let mut out: Vec<f64> = stack
        .slabs
        .iter()
        .flat_map(|s| s.loops.iter())
        .flat_map(|l| l.parts.iter())
        .filter_map(|p| match p {
            Part::Circle { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out
}

#[test]
fn a_plate_comes_back_as_one_slab_of_its_own_size() {
    let stack = slab_plan(&frozen(&plate(Vec3::new(60., 40., 10.))), &options(0.25, 0.1)).unwrap();
    assert!(stack.axis.z.abs() > 0.999, "{:?}", stack.axis);
    assert_eq!(stack.slabs.len(), 1, "{:?}", stack.levels);
    let slab = stack.slabs.first().unwrap();
    assert!((slab.to - slab.from - 10.).abs() < 0.05, "{} to {}", slab.from, slab.to);
    let outline = slab.loops.first().unwrap();
    assert!(outline.fitted, "a clean box should fit lines");
    let [width, height] = span(outline);
    assert!((width - 60.).abs() < 0.1 && (height - 40.).abs() < 0.1, "{width} x {height}");
    assert_eq!(outline.parts.len(), 4, "{:?}", outline.parts);
}

#[test]
fn a_stepped_part_comes_back_as_a_slab_for_each_level() {
    let base = plate(Vec3::new(60., 40., 10.));
    let step = crate::box_solid(Vec3::new(10., 10., 10.), Vec3::new(50., 30., 18.)).unwrap();
    let stack = slab_plan(&frozen(&crate::boolean(&base, &step, BoolOp::Union).unwrap().unwrap()), &options(0.25, 0.1)).unwrap();
    assert_eq!(stack.slabs.len(), 2, "{:?}", stack.levels);
    let (lower, upper) = (stack.slabs.first().unwrap(), stack.slabs.get(1).unwrap());
    assert!((lower.to - 10.).abs() < 0.1 && (upper.to - 18.).abs() < 0.1, "{:?}", stack.levels);
    // The lower slab is the whole plate; the upper one is only the step standing on it.
    let [wide, _] = span(lower.loops.first().unwrap());
    let [narrow, _] = span(upper.loops.first().unwrap());
    assert!((wide - 60.).abs() < 0.1, "{wide}");
    assert!((narrow - 40.).abs() < 0.1, "{narrow}");
}

#[test]
fn a_bore_comes_back_as_a_circle_inside_the_outline() {
    let body = drilled(&plate(Vec3::new(60., 40., 10.)), &[(Vec3::new(30., 20., 0.), 6.)]);
    let stack = slab_plan(&frozen(&body), &options(0.25, 0.1)).unwrap();
    let slab = stack.slabs.first().unwrap();
    assert_eq!(slab.loops.len(), 2, "an outline and the bore inside it");
    assert_eq!(circles(&stack), vec![6.], "{:?}", circles(&stack));
    // The bore turns the other way from the outline around it, which is what makes it a hole.
    let outer = slab.loops.iter().map(|l| l.area).fold(f64::NEG_INFINITY, f64::max);
    let inner = slab.loops.iter().map(|l| l.area).fold(f64::INFINITY, f64::min);
    assert!(outer > 0. && inner < 0., "{outer} and {inner}");
}

#[test]
fn a_noisy_scan_still_rounds_back_to_the_size_the_part_was_made_to() {
    // A tenth of a millimetre of scanning noise on every vertex: enough that nothing measures
    // exact, which is the case the whole module is for.
    let body = drilled(&plate(Vec3::new(60., 40., 10.)), &[(Vec3::new(30., 20., 0.), 6.)]);
    let stack = slab_plan(&scanned(&body, 0.1), &options(0.3, 0.5)).unwrap();
    let slab = stack.slabs.first().unwrap();
    assert!((slab.to - slab.from - 10.).abs() < 1e-9, "the thickness should land on the grid: {}", slab.to - slab.from);
    let outline = slab.loops.iter().max_by(|a, b| a.area.total_cmp(&b.area)).unwrap();
    let [width, height] = span(outline);
    assert!((width - 60.).abs() < 1e-6 && (height - 40.).abs() < 1e-6, "rounded to the grid: {width} x {height}");
    assert_eq!(circles(&stack), vec![6.], "the bore should round to its nominal radius: {:?}", circles(&stack));
    // And the plan should know it is close: the only error left is the noise it was given.
    let deviation = stack.deviation.unwrap();
    assert!(deviation.p95 < 0.35, "{deviation:?}");
}

#[test]
fn a_symmetric_part_finds_its_mirror_plane_and_makes_the_two_sides_match() {
    // Two bores the same distance either side of the middle, each measured differently by the
    // noise: the mirror is what should bring them back together.
    let body = drilled(&plate(Vec3::new(60., 40., 10.)), &[(Vec3::new(15., 20., 0.), 4.), (Vec3::new(45., 20., 0.), 4.)]);
    let stack = slab_plan(&scanned(&body, 0.1), &options(0.3, 0.5)).unwrap();
    assert!(!stack.mirrors.is_empty(), "a part symmetric both ways should find at least one plane");
    assert!(stack.mirrors.iter().all(|m| m.mismatch < 0.6), "{:?}", stack.mirrors);
    let bores: Vec<f64> = circles(&stack);
    assert_eq!(bores, vec![4., 4.], "both bores should come back the same size: {bores:?}");
}

#[test]
fn symmetry_can_be_turned_off() {
    let body = drilled(&plate(Vec3::new(60., 40., 10.)), &[(Vec3::new(15., 20., 0.), 4.), (Vec3::new(45., 20., 0.), 4.)]);
    let off = SlabOptions { symmetry: Symmetry::Off, ..options(0.3, 0.5) };
    assert!(slab_plan(&scanned(&body, 0.1), &off).unwrap().mirrors.is_empty());
}

#[test]
fn a_round_part_comes_back_as_one_circle() {
    let stack = slab_plan(&frozen(&crate::cylinder(Vec3::ZERO, Vec3::Z, 12., 8.).unwrap()), &options(0.25, 0.1)).unwrap();
    let slab = stack.slabs.first().unwrap();
    assert!((slab.to - slab.from - 8.).abs() < 0.05, "{} to {}", slab.from, slab.to);
    let outline = slab.loops.first().unwrap();
    assert!(matches!(outline.parts.as_slice(), [Part::Circle { radius, .. }] if (radius - 12.).abs() < 0.05), "{:?}", outline.parts);
}

#[test]
fn a_shape_that_is_not_a_stack_of_slabs_says_how_far_off_it_is() {
    // A sphere has no two levels to extrude between and no outline that holds across a layer.
    // Whatever comes out, the report has to admit it is nothing like the scan rather than offer a
    // tidy-looking part.
    let ball = crate::sphere(Vec3::ZERO, 20.).unwrap();
    match slab_plan(&frozen(&ball), &options(0.25, 0.1)) {
        Err(err) => assert!(err.to_string().contains("mesh slabs"), "{err}"),
        Ok(stack) => {
            let deviation = stack.deviation.expect("a plan should always report how close it came");
            assert!(deviation.p95 > 1., "a sphere read as slabs should be far off, not {deviation:?}");
        }
    }
}

#[test]
fn the_build_axis_can_be_given() {
    // The plate is thinnest in z, so left alone the plan builds along z. Asked for x, it has to
    // build along x instead and read the part as a 40 by 10 profile 60 long.
    let stack = slab_plan(&frozen(&plate(Vec3::new(60., 40., 10.))), &SlabOptions { axis: Some(Vec3::X), ..options(0.25, 0.1) }).unwrap();
    assert!(stack.axis.x.abs() > 0.999, "{:?}", stack.axis);
    let slab = stack.slabs.first().unwrap();
    assert!((slab.to - slab.from - 60.).abs() < 0.1, "{} to {}", slab.from, slab.to);
}

#[test]
fn hostile_options_are_refused_rather_than_guessed_at() {
    let body = frozen(&plate(Vec3::new(60., 40., 10.)));
    let refused = |o: SlabOptions| slab_plan(&body, &o).unwrap_err().to_string();
    assert!(refused(options(0., 0.1)).contains("tolerance"));
    assert!(refused(options(f64::NAN, 0.1)).contains("tolerance"));
    assert!(refused(options(0.25, -1.)).contains("grid"));
    assert!(refused(SlabOptions { snap_angle: f64::INFINITY, ..options(0.25, 0.1) }).contains("snap_angle"));
    assert!(refused(SlabOptions { min_thickness: 0., ..options(0.25, 0.1) }).contains("min_thickness"));
    assert!(refused(SlabOptions { axis: Some(Vec3::ZERO), ..options(0.25, 0.1) }).contains("zero length"));
}

#[test]
fn a_body_that_is_not_a_mesh_is_refused() {
    let err = slab_plan(&plate(Vec3::new(10., 10., 10.)), &SlabOptions::default()).unwrap_err().to_string();
    assert!(err.contains("select a mesh body"), "{err}");
}
