//! The SolidCraft fork's tangency-labs/smlib@2e1bb8c under
//! `Kernel::fixes.offset_four_face_vertex_split` with `offset_closes_loop_gaps` (WR-666,
//! #2052), on the commit message's two cases, rebuilt:
//!
//! - a trough 60 x 40 x 20 whose floor is four facets falling to a drain at its middle (the
//!   long sides' facets fall 1 degree, the short sides' 2/3 degree, adjacent facets meet at
//!   about 1.2 degrees), shelled 2 inward open at the top;
//! - an 80 x 60 x 20 slab with that floor's pocket cut 10 deep into its top, shelled 3
//!   inward open at the top,
//!
//! both as SolidCraft's `smlib_shell` calls `SmApiShellBrepFull(b, -t, TRUE, TRUE, TRUE,
//! [top])`. Four faces meet at each corner of the floor (two walls, two facets) and at the
//! drain (four facets), every edge there convex (the trough) or concave (the pocket), and
//! their four offset planes do not meet in one point: each offset vertex is two, joined by
//! a new edge. Upstream refuses the trough (1001) and returns the pocket at the wrong
//! volume; with the opt-in both shell to the closed form (each cavity is the intersection
//! or difference of the faces' half-spaces moved by the thickness, integrated apart from
//! the kernel), within 1e-15 relative here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{api_shell, planar_faces_at, volume};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{create_planar_face_with_3d_curves, FaceBuildEnv};
use solidcore::topology::topology_traverser::Marks;
use solidcore::{Brep, Kernel};

const LX: f64 = 60.0;
const LY: f64 = 40.0;
const H: f64 = 20.0;
const T: f64 = 2.0;

/// The drain's depth below the bottom edges: the long sides' facets fall 1 degree.
fn drain_depth() -> f64 {
    (LY / 2.0) * 1.0_f64.to_radians().tan()
}

/// A solid from planar polygons (each a closed loop of corner points), stitched and
/// oriented, as `SmApiCreatePyramid` builds its faces.
fn polyhedron(k: &mut Kernel, faces: &[Vec<Point3>], holed: &[(Vec<Point3>, Vec<Point3>)]) -> Brep {
    let mut brep = solidcore::topology::brep::p3::new_brep(k, -1.0, -1.0).unwrap();
    let region = brep.infinite_region();
    let mut state = ArenaNotifyState::new(&brep);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face =
        |f| solidcore::topology::loopuse::notify_ref(solidcore::error::EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let lines = |corners: &[Point3]| -> Vec<Curve> {
        (0..corners.len())
            .map(|i| {
                Curve::from(
                    Line::create_line_segment(
                        3,
                        corners[i],
                        corners[(i + 1) % corners.len()],
                        None,
                    )
                    .unwrap(),
                )
            })
            .collect()
    };
    let loops: Vec<Vec<Curve>> = faces
        .iter()
        .map(|f| lines(f))
        .chain(holed.iter().map(|(outer, hole)| {
            let mut c = lines(outer);
            c.extend(lines(hole));
            c
        }))
        .collect();
    for curves in &loops {
        create_planar_face_with_3d_curves(k, &mut brep, region, curves, 1.0e-5, &mut fenv)
            .unwrap()
            .unwrap();
    }
    stitch_and_orient(k, &mut brep, &StitchAndOrientOptions::default()).unwrap();
    brep.refresh_cached_topology();
    brep
}

/// The four facets of a floor over the rectangle `[x0, x1] x [y0, y1]` at height `zb`,
/// each through one side and the drain `drain_depth()` below the middle, each moved by `d`
/// along its upward normal, as `z = a x + b y + c`.
fn floor_planes(x0: f64, x1: f64, y0: f64, y1: f64, zb: f64, d: f64) -> [[f64; 3]; 4] {
    let h = drain_depth();
    let (sx, sy) = (h / ((x1 - x0) / 2.0), h / ((y1 - y0) / 2.0));
    let (ox, oy) = (d * (1.0 + sx * sx).sqrt(), d * (1.0 + sy * sy).sqrt());
    [
        [0.0, -sy, zb + sy * y0 + oy],
        [0.0, sy, zb - sy * y1 + oy],
        [-sx, 0.0, zb + sx * x0 + ox],
        [sx, 0.0, zb - sx * x1 + ox],
    ]
}

/// The floor's facets (corner points) over `[x0, x1] x [y0, y1]` at `zb`.
fn facets(x0: f64, x1: f64, y0: f64, y1: f64, zb: f64) -> Vec<Vec<Point3>> {
    let b = [
        Point3::new(x0, y0, zb),
        Point3::new(x1, y0, zb),
        Point3::new(x1, y1, zb),
        Point3::new(x0, y1, zb),
    ];
    let drain = Point3::new((x0 + x1) / 2.0, (y0 + y1) / 2.0, zb - drain_depth());
    (0..4).map(|i| vec![b[(i + 1) % 4], b[i], drain]).collect()
}

/// The upright walls over the sides of `[x0, x1] x [y0, y1]` from `z0` to `z1`.
fn walls(x0: f64, x1: f64, y0: f64, y1: f64, z0: f64, z1: f64) -> Vec<Vec<Point3>> {
    let c = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    (0..4)
        .map(|i| {
            let (p, q) = (c[i], c[(i + 1) % 4]);
            vec![
                Point3::new(p[0], p[1], z0),
                Point3::new(q[0], q[1], z0),
                Point3::new(q[0], q[1], z1),
                Point3::new(p[0], p[1], z1),
            ]
        })
        .collect()
}

fn rect(x0: f64, x1: f64, y0: f64, y1: f64, z: f64) -> Vec<Point3> {
    vec![
        Point3::new(x0, y0, z),
        Point3::new(x1, y0, z),
        Point3::new(x1, y1, z),
        Point3::new(x0, y1, z),
    ]
}

/// The trough: four walls from z = 0 to the top at `H`, the floor four facets each
/// through one bottom edge and the drain below the middle.
fn trough(k: &mut Kernel) -> Brep {
    let mut faces = vec![rect(0.0, LX, 0.0, LY, H)];
    faces.extend(walls(0.0, LX, 0.0, LY, 0.0, H));
    faces.extend(facets(0.0, LX, 0.0, LY, 0.0));
    polyhedron(k, &faces, &[])
}

/// The pocket's slab, 80 x 60 x 20, and the pocket cut 10 deep into its top over
/// `[10, 70] x [10, 50]` (the trough's 60 x 40), its floor four facets falling to a drain.
const SLAB: [f64; 3] = [80.0, 60.0, 20.0];
const POCKET: [f64; 4] = [10.0, 70.0, 10.0, 50.0];
const POCKET_FLOOR: f64 = 10.0;
const POCKET_T: f64 = 3.0;

fn pocket(k: &mut Kernel) -> Brep {
    let [px0, px1, py0, py1] = POCKET;
    let (sx, sy, sz) = (SLAB[0], SLAB[1], SLAB[2]);
    let mut faces = vec![rect(0.0, sx, 0.0, sy, 0.0)];
    faces.extend(walls(0.0, sx, 0.0, sy, 0.0, sz));
    faces.extend(walls(px0, px1, py0, py1, POCKET_FLOOR, sz));
    faces.extend(facets(px0, px1, py0, py1, POCKET_FLOOR));
    let hole = rect(px0, px1, py0, py1, sz);
    polyhedron(k, &faces, &[(rect(0.0, sx, 0.0, sy, sz), hole)])
}

/// Sutherland-Hodgman clip of `poly` to `a x + b y + c >= 0`.
fn clip(poly: &[[f64; 2]], a: f64, b: f64, c: f64) -> Vec<[f64; 2]> {
    let f = |q: [f64; 2]| a * q[0] + b * q[1] + c;
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (fp, fq) = (f(p), f(q));
        if fp >= 0.0 {
            out.push(p);
        }
        if (fp >= 0.0) != (fq >= 0.0) {
            let s = fp / (fp - fq);
            out.push([p[0] + s * (q[0] - p[0]), p[1] + s * (q[1] - p[1])]);
        }
    }
    out
}

/// The integral over `[x0, x1] x [y0, y1]` of `top` less the greatest of `planes`: each
/// plane over the polygon where it is the greatest (area times its value at the centroid).
fn under(top: f64, x0: f64, x1: f64, y0: f64, y1: f64, planes: &[[f64; 3]; 4]) -> f64 {
    let rect = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let mut total = 0.0;
    for (i, pi) in planes.iter().enumerate() {
        let mut poly = rect.clone();
        for (j, pj) in planes.iter().enumerate() {
            if i != j {
                poly = clip(&poly, pi[0] - pj[0], pi[1] - pj[1], pi[2] - pj[2]);
            }
        }
        if poly.len() < 3 {
            continue;
        }
        let (mut area2, mut cx, mut cy) = (0.0, 0.0, 0.0);
        for k in 0..poly.len() {
            let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
            let cross = p[0] * q[1] - q[0] * p[1];
            area2 += cross;
            cx += (p[0] + q[0]) * cross;
            cy += (p[1] + q[1]) * cross;
        }
        let (cx, cy) = (cx / (3.0 * area2), cy / (3.0 * area2));
        total += area2 / 2.0 * (top - (pi[0] * cx + pi[1] * cy + pi[2]));
    }
    total
}

fn trough_solid_volume() -> f64 {
    LX * LY * H + LX * LY * drain_depth() / 3.0
}

/// The shelled trough: the solid less the cavity, the region inside the walls moved in by
/// `T`, below the top and above the four facets' planes moved in by `T` along their
/// normals (the cavity is convex: the intersection of those half-spaces).
fn trough_shell_volume() -> f64 {
    let planes = floor_planes(0.0, LX, 0.0, LY, 0.0, T);
    trough_solid_volume() - under(H, T, LX - T, T, LY - T, &planes)
}

fn pocket_volume(depth_below_top: f64, area: f64) -> f64 {
    area * depth_below_top + area * drain_depth() / 3.0
}

fn pocket_solid_volume() -> f64 {
    let [px0, px1, py0, py1] = POCKET;
    SLAB[0] * SLAB[1] * SLAB[2] - pocket_volume(SLAB[2] - POCKET_FLOOR, (px1 - px0) * (py1 - py0))
}

/// The shelled slab: the solid less the cavity, the slab's box moved in by `POCKET_T`
/// (open at the top) less the pocket grown by `POCKET_T` (its walls moved out, its
/// facets' planes moved down along their normals, up to the open top).
fn pocket_shell_volume() -> f64 {
    let [px0, px1, py0, py1] = POCKET;
    let t = POCKET_T;
    let boxed = (SLAB[0] - 2.0 * t) * (SLAB[1] - 2.0 * t) * (SLAB[2] - t);
    let planes = floor_planes(px0, px1, py0, py1, POCKET_FLOOR, -t);
    let grown = under(SLAB[2], px0 - t, px1 + t, py0 - t, py1 + t, &planes);
    pocket_solid_volume() - (boxed - grown)
}

#[derive(Debug, PartialEq)]
struct Shelled {
    volume: f64,
    faces: usize,
    manifold: bool,
}

/// `part` shelled `t` inward open at its top (the plane face at `top`) under `fixes`, as
/// SolidCraft's `smlib_shell` calls `SmApiShellBrepFull`.
fn shell(
    fixes: &UpstreamFixes,
    make: fn(&mut Kernel) -> Brep,
    top: f64,
    t: f64,
) -> Result<Shelled, String> {
    let mut k = Kernel::default();
    k.fixes.clone_from(fixes);
    let mut part = make(&mut k);
    assert!(brep_is_manifold_solid(&part).unwrap());
    let open = planar_faces_at(&k, &part, 2, top);
    assert_eq!(open.len(), 1);
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -t,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: false,
    };
    let mut wall = api_shell(&mut k, &mut part, options, &open).map_err(|e| e.to_string())?;
    wall.refresh_cached_topology();
    let manifold = brep_is_manifold_solid(&wall).unwrap();
    let volume = volume(&k, &mut wall).map_err(|e| e.to_string())?;
    Ok(Shelled {
        volume,
        faces: wall.face_list().len(),
        manifold,
    })
}

fn shell_trough(fixes: &UpstreamFixes) -> Result<Shelled, String> {
    shell(fixes, trough, H, T)
}

fn shell_pocket(fixes: &UpstreamFixes) -> Result<Shelled, String> {
    shell(fixes, pocket, SLAB[2], POCKET_T)
}

/// The opt-in with the loop-gap closing it builds on (`offset_closes_loop_gaps`, WR-578).
fn split() -> UpstreamFixes {
    let mut f = gaps_only();
    f.offset_four_face_vertex_split = true;
    f
}

fn gaps_only() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.offset_closes_loop_gaps = true;
    f
}

fn assert_closed_form(got: &Shelled, expected: f64, faces: usize, label: &str) {
    assert!(got.manifold, "{label}: {got:?}");
    assert_eq!(got.faces, faces, "{label}: {got:?}");
    assert!(
        (got.volume - expected).abs() <= 1e-9 * expected,
        "{label}: {} vs closed form {expected}",
        got.volume
    );
}

#[test]
fn the_parts_have_their_closed_form_volumes() {
    let mut k = Kernel::default();
    let mut b = trough(&mut k);
    assert_eq!(b.face_list().len(), 9);
    let v = volume(&k, &mut b).unwrap();
    assert!((v - trough_solid_volume()).abs() <= 1e-9 * v, "{v}");
    let mut b = pocket(&mut k);
    assert_eq!(b.face_list().len(), 14);
    let v = volume(&k, &mut b).unwrap();
    assert!((v - pocket_solid_volume()).abs() <= 1e-9 * v, "{v}");
}

/// Upstream (the default fixes) refuses the trough's shell: the merge fails (1001).
#[test]
fn default_fixes_refuse_the_trough_shell() {
    let got = shell_trough(&UpstreamFixes::default());
    assert_eq!(
        got,
        Err("boolean::merge::p2::make_faceuse: upstream status 1001".to_owned())
    );
}

/// The loop-gap closing alone does not build it either: the corners and the drain are
/// still one vertex each.
#[test]
fn loop_gap_closing_alone_refuses_the_trough_shell() {
    let got = shell_trough(&gaps_only());
    assert!(got.is_err(), "{got:?}");
}

/// The flag without `offset_closes_loop_gaps` has no effect.
#[test]
fn the_flag_alone_is_the_default() {
    let mut f = UpstreamFixes::default();
    f.offset_four_face_vertex_split = true;
    assert_eq!(shell_trough(&f), shell_trough(&UpstreamFixes::default()));
    assert_eq!(shell_pocket(&f), shell_pocket(&UpstreamFixes::default()));
}

/// With the opt-in the trough (every edge at its corners and drain convex) shells to the
/// closed form: the 8 outer faces, their 8 offsets and the rim.
#[test]
fn trough_shells_to_its_closed_form() {
    let got = shell_trough(&split()).unwrap();
    assert_closed_form(&got, trough_shell_volume(), 17, "trough");
}

/// With the opt-in the pocket (every edge at its corners and drain concave) shells to the
/// closed form: the slab's 13 faces below the top, their 13 offsets and the two rims (round the outside and round the pocket).
#[test]
fn pocket_shells_to_its_closed_form() {
    let got = shell_pocket(&split()).unwrap();
    assert_closed_form(&got, pocket_shell_volume(), 28, "pocket");
}

/// Upstream (the default fixes) returns the pocket's shell, a manifold solid, at the wrong
/// volume (43302.668, the closed form 43522.321); the loop-gap closing alone refuses it.
#[test]
fn without_the_flag_the_pocket_shell_is_wrong_or_refused() {
    let got = shell_pocket(&UpstreamFixes::default()).unwrap();
    assert!(
        (got.volume - pocket_shell_volume()).abs() > 100.0,
        "{got:?} vs {}",
        pocket_shell_volume()
    );
    assert!(shell_pocket(&gaps_only()).is_err());
}
