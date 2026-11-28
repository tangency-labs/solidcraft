//! The OCCT adapter's row of the kernel support matrix (`docs/kernel-support.md`): every case of
//! the operations it takes, built through the router on solidcore and on Open CASCADE, graded
//! against its closed-form volume and area and by the shared check.
//!
//! `cargo run --release -p solidcraft-kernel --features backend-occt --example occt_matrix`
//! (add `-- --json <file>` for the full results). Only builds with the feature have it.
//!
//! A case passes on a kernel when the kernel builds it, the shared check passes the result, and
//! its volume and area are within 1e-9 (relative) of the case's answer; a case whose answer is a
//! refusal or an empty result passes when the kernel answers so. On Open CASCADE a case is
//! `unsupported` when the router ran it on solidcore instead (the adapter doesn't take the
//! operation or the operand, or the result doesn't cross back as one record). The accuracy
//! columns compare the two kernels on the same case: the worst edge or vertex tolerance the
//! result's record carries, and the volume and area errors against the answer. A case counts as
//! looser when one of them is larger on Open CASCADE than on solidcore by more than 1e-15 (the
//! last bits of a double), and is counted again when it is also above 1e-12.

use std::f64::consts::PI;
use std::time::Instant;

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::foreign::{self, Kernel};
use solidcraft_kernel::{Body, BoolOp, Result};

/// Relative error a pass allows in volume and area.
const EXACT: f64 = 1e-9;

/// What a case should give.
enum Answer {
    /// A solid of this volume and area (`None`: area not derived for this case).
    Solid { volume: f64, area: Option<f64> },
    /// A sheet (a surface body) of this area.
    Sheet { area: f64 },
    /// Nothing left.
    Empty,
    /// A refusal.
    Refused,
}

struct Case {
    op: &'static str,
    name: &'static str,
    build: fn() -> Result<Option<Body>>,
    answer: Answer,
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn some(b: Result<Body>) -> Result<Option<Body>> {
    b.map(Some)
}

/// The one body of a profile operation.
fn one(b: Result<Vec<Body>>) -> Result<Option<Body>> {
    let mut b = b?;
    Ok(if b.len() == 1 { b.pop() } else { None })
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)])
}

/// Piece `i` of a split.
fn piece(b: Result<Vec<Body>>, i: usize) -> Result<Option<Body>> {
    Ok(b?.into_iter().nth(i))
}

/// A split of `body` by the plane through `origin` square to `normal`.
fn split(body: Result<Body>, origin: Vec3, normal: Vec3) -> Result<Vec<Body>> {
    use solidcraft_geom::PlaneGeom as _;
    let plane = Plane::from_normal(origin, normal).ok_or_else(|| solidcraft_kernel::KernelError::Invalid("no plane".into()))?;
    solidcraft_kernel::split_body(&body?, &solidcraft_kernel::SplitTool::Plane(plane))
}

/// A 10 square sheet on XY (with a hole of r 2 in its middle when `holed`), trimmed by the plane
/// x = 3 (or x = 5 with the hole), keeping the side of x = 9.
fn trimmed(holed: bool) -> Result<Option<Body>> {
    use solidcraft_geom::PlaneGeom as _;
    let holes = if holed { vec![Loop2::circle(Vec2::new(5.0, 5.0), 2.0)] } else { vec![] };
    let sheet = solidcraft_kernel::patch_region(&Plane::XY, &Region2 { outer: rect(0.0, 0.0, 10.0, 10.0), holes })?;
    let plane = Plane::from_normal(v(if holed { 5.0 } else { 3.0 }, 0.0, 0.0), v(1.0, 0.0, 0.0))
        .ok_or_else(|| solidcraft_kernel::KernelError::Invalid("no plane".into()))?;
    some(solidcraft_kernel::trim_keep(&sheet, &solidcraft_kernel::SplitTool::Plane(plane), solidcraft_kernel::TrimKeep::Side(v(9.0, 5.0, 0.0))))
}

fn cube(lo: f64, hi: f64) -> Result<Body> {
    solidcraft_kernel::box_solid(v(lo, lo, lo), v(hi, hi, hi))
}

/// The volume and area a constant round of radius `r` takes off a convex right-angled edge of
/// length `l` whose ends are flat faces: the section r² − πr²/4 along the edge; the two strips r
/// wide traded for a quarter cylinder, and the section off each end face.
fn edge_round(l: f64, r: f64) -> (f64, f64) {
    let section = r * r * (1.0 - PI / 4.0);
    (l * section, 2.0 * l * r - l * PI * r / 2.0 + 2.0 * section)
}

/// A round of radius `r` on the top rim of a cylinder of radius `big`, by Pappus: what it takes
/// off the volume and the area.
fn rim_round(big: f64, r: f64) -> (f64, f64) {
    // The section: the square of side r at the rim less the quarter disc about (big − r, h − r).
    let square = r * r * (big - r / 2.0);
    let disc = PI * r * r / 4.0 * (big - r + 4.0 * r / (3.0 * PI));
    let volume = 2.0 * PI * (square - disc);
    // Off the top disc and the side; on, the quarter torus (arc length πr/2 about its centroid).
    let off = PI * (big * big - (big - r) * (big - r)) + 2.0 * PI * big * r;
    let on = 2.0 * PI * (big - r + 2.0 * r / PI) * (PI * r / 2.0);
    (volume, off - on)
}

fn cases() -> Vec<Case> {
    let (e1v, e1a) = edge_round(10.0, 1.0);
    let (rimv, rima) = rim_round(2.0, 0.5);
    vec![
        Case {
            op: "box",
            name: "1×2×3 at the origin",
            build: || some(solidcraft_kernel::box_solid(v(0.0, 0.0, 0.0), v(1.0, 2.0, 3.0))),
            answer: Answer::Solid { volume: 6.0, area: Some(22.0) },
        },
        Case {
            op: "box",
            name: "10 cube about the origin, corners given high to low",
            build: || some(solidcraft_kernel::box_solid(v(5.0, 5.0, 5.0), v(-5.0, -5.0, -5.0))),
            answer: Answer::Solid { volume: 1000.0, area: Some(600.0) },
        },
        Case {
            op: "box",
            name: "degenerate (flat)",
            build: || some(solidcraft_kernel::box_solid(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0))),
            answer: Answer::Refused,
        },
        Case {
            op: "cylinder",
            name: "r 2, h 3 along z",
            build: || some(solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 2.0, 3.0)),
            answer: Answer::Solid { volume: 12.0 * PI, area: Some(20.0 * PI) },
        },
        Case {
            op: "cylinder",
            name: "r 1.5, h 4 along (1, 1, 1)",
            build: || some(solidcraft_kernel::cylinder(v(1.0, -2.0, 0.5), v(1.0, 1.0, 1.0), 1.5, 4.0)),
            answer: Answer::Solid { volume: 9.0 * PI, area: Some(16.5 * PI) },
        },
        Case {
            op: "cylinder",
            name: "radius 0",
            build: || some(solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 0.0, 3.0)),
            answer: Answer::Refused,
        },
        Case {
            op: "sphere",
            name: "r 1.5 at the origin",
            build: || some(solidcraft_kernel::sphere(v(0.0, 0.0, 0.0), 1.5)),
            answer: Answer::Solid { volume: 4.5 * PI, area: Some(9.0 * PI) },
        },
        Case {
            op: "sphere",
            name: "r 100 off the origin",
            build: || some(solidcraft_kernel::sphere(v(10.0, 20.0, 30.0), 100.0)),
            answer: Answer::Solid { volume: 4.0 / 3.0 * PI * 1e6, area: Some(4.0 * PI * 1e4) },
        },
        Case {
            op: "torus",
            name: "R 3, r 1",
            build: || some(solidcraft_kernel::torus(v(0.0, 0.0, 0.0), 3.0, 1.0)),
            answer: Answer::Solid { volume: 6.0 * PI * PI, area: Some(12.0 * PI * PI) },
        },
        Case {
            op: "torus",
            name: "R 10, r 0.5 off the origin",
            build: || some(solidcraft_kernel::torus(v(1.0, 2.0, 3.0), 10.0, 0.5)),
            answer: Answer::Solid { volume: 5.0 * PI * PI, area: Some(20.0 * PI * PI) },
        },
        Case {
            op: "torus",
            name: "minor above major",
            build: || some(solidcraft_kernel::torus(v(0.0, 0.0, 0.0), 1.0, 2.0)),
            answer: Answer::Refused,
        },
        Case {
            op: "extrude",
            name: "a 4×3 rectangle, 5 up",
            build: || one(solidcraft_kernel::extrude(&Plane::XY, &[Region2 { outer: rect(0.0, 0.0, 4.0, 3.0), holes: vec![] }], 0.0, 5.0)),
            answer: Answer::Solid { volume: 60.0, area: Some(94.0) },
        },
        Case {
            op: "extrude",
            name: "a 10 plate with an r 2 hole, 3 thick, from -1",
            build: || {
                let r = Region2 { outer: rect(0.0, 0.0, 10.0, 10.0), holes: vec![Loop2::circle(Vec2::new(5.0, 5.0), 2.0)] };
                one(solidcraft_kernel::extrude(&Plane::XY, &[r], -1.0, 2.0))
            },
            answer: Answer::Solid { volume: 300.0 - 12.0 * PI, area: Some(2.0 * (100.0 - 4.0 * PI) + 120.0 + 12.0 * PI) },
        },
        Case {
            op: "extrude",
            name: "an r 3 disc on a tilted plane, 4 long, whole sides",
            build: || {
                let pl = Plane { origin: v(1.0, 2.0, 3.0), x: v(0.6, 0.8, 0.0), y: v(0.0, 0.0, 1.0) };
                let r = Region2 { outer: Loop2::circle(Vec2::new(1.0, 2.0), 3.0), holes: vec![] };
                one(solidcraft_kernel::extrude_whole_sides(&pl, &[r], 0.0, 4.0, &|_, _| true))
            },
            answer: Answer::Solid { volume: 36.0 * PI, area: Some(18.0 * PI + 24.0 * PI) },
        },
        Case {
            op: "extrude",
            name: "zero distance",
            build: || one(solidcraft_kernel::extrude(&Plane::XY, &[Region2 { outer: rect(0.0, 0.0, 1.0, 1.0), holes: vec![] }], 1.0, 1.0)),
            answer: Answer::Refused,
        },
        Case {
            op: "revolve",
            name: "a 2×3 rectangle 2 off the axis, a full turn",
            build: || {
                let r = Region2 { outer: rect(2.0, 0.0, 4.0, 3.0), holes: vec![] };
                one(solidcraft_kernel::revolve_whole(&Plane::XZ, &[r], Vec2::ZERO, Vec2::new(0.0, 1.0), 2.0 * PI))
            },
            // Pappus: the section 6 about its centroid at 3; the walls 2π·4·3 and 2π·2·3, the
            // two rings π(16 − 4).
            answer: Answer::Solid { volume: 36.0 * PI, area: Some(24.0 * PI + 12.0 * PI + 24.0 * PI) },
        },
        Case {
            op: "revolve",
            name: "an r 2, 5 tall rectangle on the axis, a quarter turn",
            build: || {
                let r = Region2 { outer: rect(0.0, 0.0, 2.0, 5.0), holes: vec![] };
                one(solidcraft_kernel::revolve(&Plane::XZ, &[r], Vec2::ZERO, Vec2::new(0.0, 1.0), PI / 2.0))
            },
            answer: Answer::Solid { volume: 5.0 * PI, area: Some(2.0 * PI + 20.0 + 5.0 * PI) },
        },
        Case {
            op: "revolve",
            name: "a zero turn",
            build: || {
                let r = Region2 { outer: rect(2.0, 0.0, 4.0, 3.0), holes: vec![] };
                one(solidcraft_kernel::revolve(&Plane::XZ, &[r], Vec2::ZERO, Vec2::new(0.0, 1.0), 0.0))
            },
            answer: Answer::Refused,
        },
        Case {
            op: "boolean",
            name: "2 cube cut by a 2 cube at its corner",
            build: || solidcraft_kernel::boolean(&cube(0.0, 2.0)?, &cube(1.0, 3.0)?, BoolOp::Cut),
            answer: Answer::Solid { volume: 7.0, area: Some(24.0) },
        },
        Case {
            op: "boolean",
            name: "2 cube with a 2 cube at its corner",
            build: || solidcraft_kernel::boolean(&cube(0.0, 2.0)?, &cube(1.0, 3.0)?, BoolOp::Union),
            answer: Answer::Solid { volume: 15.0, area: Some(42.0) },
        },
        Case {
            op: "boolean",
            name: "2 cube and a 2 cube at its corner, in common",
            build: || solidcraft_kernel::boolean(&cube(0.0, 2.0)?, &cube(1.0, 3.0)?, BoolOp::Intersect),
            answer: Answer::Solid { volume: 1.0, area: Some(6.0) },
        },
        Case {
            op: "boolean",
            name: "a through hole: 10 cube less an r 2 cylinder",
            build: || {
                solidcraft_kernel::boolean(
                    &cube(0.0, 10.0)?,
                    &solidcraft_kernel::cylinder(v(5.0, 5.0, -1.0), v(0.0, 0.0, 1.0), 2.0, 12.0)?,
                    BoolOp::Cut,
                )
            },
            answer: Answer::Solid { volume: 1000.0 - 40.0 * PI, area: Some(600.0 + 32.0 * PI) },
        },
        Case {
            op: "boolean",
            name: "the lower half of an r 2 sphere",
            build: || {
                solidcraft_kernel::boolean(
                    &solidcraft_kernel::sphere(v(0.0, 0.0, 0.0), 2.0)?,
                    &solidcraft_kernel::box_solid(v(-3.0, -3.0, 0.0), v(3.0, 3.0, 3.0))?,
                    BoolOp::Cut,
                )
            },
            answer: Answer::Solid { volume: 16.0 * PI / 3.0, area: Some(12.0 * PI) },
        },
        Case {
            op: "boolean",
            name: "two crossing r 1 cylinders, joined",
            build: || {
                let a = solidcraft_kernel::cylinder(v(-3.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, 6.0)?;
                let b = solidcraft_kernel::cylinder(v(0.0, -3.0, 0.0), v(0.0, 1.0, 0.0), 1.0, 6.0)?;
                solidcraft_kernel::boolean(&a, &b, BoolOp::Union)
            },
            // Each is 6π; they share a bicylinder of 16/3, whose 16 of surface is half in each.
            answer: Answer::Solid { volume: 12.0 * PI - 16.0 / 3.0, area: Some(28.0 * PI - 16.0) },
        },
        Case {
            op: "boolean",
            name: "a cut that removes everything",
            build: || solidcraft_kernel::boolean(&cube(0.0, 1.0)?, &cube(-1.0, 2.0)?, BoolOp::Cut),
            answer: Answer::Empty,
        },
        Case {
            op: "boolean",
            name: "apart cubes in common",
            build: || solidcraft_kernel::boolean(&cube(0.0, 1.0)?, &cube(2.0, 3.0)?, BoolOp::Intersect),
            answer: Answer::Empty,
        },
        Case {
            op: "boolean",
            name: "apart cubes joined",
            build: || solidcraft_kernel::boolean(&cube(0.0, 1.0)?, &cube(2.0, 3.0)?, BoolOp::Union),
            answer: Answer::Solid { volume: 2.0, area: Some(12.0) },
        },
        Case {
            op: "fillet",
            name: "one edge of a 10 cube, r 1",
            build: || some(solidcraft_kernel::fillet(&cube(0.0, 10.0)?, &[v(5.0, 0.0, 10.0)], 1.0)),
            answer: Answer::Solid { volume: 1000.0 - e1v, area: Some(600.0 - e1a) },
        },
        Case {
            op: "fillet",
            name: "the four top edges of a 10 cube, r 1 (mitred)",
            build: || {
                some(solidcraft_kernel::fillet(
                    &cube(0.0, 10.0)?,
                    &[v(5.0, 0.0, 10.0), v(10.0, 5.0, 10.0), v(5.0, 10.0, 10.0), v(0.0, 5.0, 10.0)],
                    1.0,
                ))
            },
            // A mitred frame: each side takes ∫(l − 2u) dA over the section, u its distance in
            // from the edge; ∫u dA = 1/2 − (π/4)(1 − 4/(3π)) = 5/6 − π/4 at r 1.
            answer: Answer::Solid { volume: 1000.0 - 4.0 * (10.0 * (1.0 - PI / 4.0) - 2.0 * (5.0 / 6.0 - PI / 4.0)), area: None },
        },
        Case {
            op: "fillet",
            name: "the top rim of an r 2 cylinder, r 0.5",
            build: || {
                some(solidcraft_kernel::fillet(&solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 2.0, 3.0)?, &[v(2.0, 0.0, 3.0)], 0.5))
            },
            answer: Answer::Solid { volume: 12.0 * PI - rimv, area: Some(20.0 * PI - rima) },
        },
        Case {
            op: "revolve",
            name: "a disc of r 1 at radius 6 drawn as two half circles, arcs joined, a full turn",
            build: || {
                let half = |start: f64| Seg2::Arc { center: Vec2::new(6.0, 0.0), radius: 1.0, start, sweep: PI };
                let r = Region2 { outer: Loop2 { segs: vec![half(0.0), half(PI)] }, holes: vec![] };
                one(solidcraft_kernel::revolve_joining_arcs(&Plane::XZ, &[r], Vec2::ZERO, Vec2::new(0.0, 1.0), 2.0 * PI))
            },
            // A ring torus: 2π · 6 · π r², area 4π² · 6 · r.
            answer: Answer::Solid { volume: 12.0 * PI * PI, area: Some(24.0 * PI * PI) },
        },
        Case {
            op: "fillet",
            name: "one edge of a 10 cube, r 1, as the Fillet feature rounds it",
            build: || {
                some(
                    solidcraft_kernel::fillet_removing(&cube(0.0, 10.0)?, &[v(5.0, 0.0, 10.0)], 1.0, solidcraft_kernel::FilletTurn::Mitre)
                        .map(|(b, _)| b),
                )
            },
            answer: Answer::Solid { volume: 1000.0 - e1v, area: Some(600.0 - e1a) },
        },
        Case {
            op: "chamfer",
            name: "one edge of a 10 cube, 1 back",
            build: || some(solidcraft_kernel::chamfer(&cube(0.0, 10.0)?, &[v(5.0, 0.0, 10.0)], 1.0)),
            // A prism of section 1/2 off: two 1-wide strips and two end triangles of 1/2 traded for one √2 wide.
            answer: Answer::Solid { volume: 995.0, area: Some(600.0 - 20.0 - 1.0 + 10.0 * 2f64.sqrt()) },
        },
        Case {
            op: "shell",
            name: "a 10 cube, 1 thick, open at the top",
            build: || some(solidcraft_kernel::shell(&cube(0.0, 10.0)?, &[v(5.0, 5.0, 10.0)], 1.0)),
            // The 8 x 8 x 9 hollow out; outside 500, inside 64 + 4 · 72, the rim 100 − 64.
            answer: Answer::Solid { volume: 424.0, area: Some(500.0 + 64.0 + 288.0 + 36.0) },
        },
        Case {
            op: "thicken",
            name: "a 10 square sheet, 2 up",
            build: || {
                some(solidcraft_kernel::thicken(
                    &solidcraft_kernel::patch_region(&Plane::XY, &Region2 { outer: rect(0.0, 0.0, 10.0, 10.0), holes: vec![] })?,
                    2.0,
                    false,
                ))
            },
            answer: Answer::Solid { volume: 200.0, area: Some(280.0) },
        },
        Case {
            op: "offset faces",
            name: "the top of a 10 cube moved out 2",
            build: || some(solidcraft_kernel::offset_faces(&cube(0.0, 10.0)?, &[v(5.0, 5.0, 10.0)], 2.0)),
            answer: Answer::Solid { volume: 1200.0, area: Some(680.0) },
        },
        Case {
            op: "draft",
            name: "a side of a 10 cube tilted 0.1 rad in going up from z 0",
            build: || some(solidcraft_kernel::draft(&cube(0.0, 10.0)?, &[v(10.0, 5.0, 5.0)], &Plane::XY, v(0.0, 0.0, 1.0), 0.1)),
            // The cube less a wedge of section 10 · 10 tan(0.1) / 2, 10 deep (area not derived).
            answer: Answer::Solid { volume: 1000.0 - 500.0 * 0.1_f64.tan(), area: None },
        },
        Case {
            op: "loft",
            name: "a 4 square to a 2 square, 5 up",
            build: || {
                let at = |z: f64| Plane { origin: v(0.0, 0.0, z), ..Plane::XY };
                some(solidcraft_kernel::loft(&[(at(0.0), rect(-2.0, -2.0, 2.0, 2.0)), (at(5.0), rect(-1.0, -1.0, 1.0, 1.0))]))
            },
            // A frustum: h/3 (A + B + √(AB)); the four trapezoid sides (4 + 2)/2 · √(25 + 1) each.
            answer: Answer::Solid { volume: 5.0 / 3.0 * (16.0 + 4.0 + 8.0), area: Some(16.0 + 4.0 + 4.0 * 3.0 * 26f64.sqrt()) },
        },
        Case {
            op: "loft",
            name: "a 4 square to a point 3 above its centre",
            build: || some(solidcraft_kernel::loft_to_point(&Plane::XY, &rect(-2.0, -2.0, 2.0, 2.0), v(0.0, 0.0, 3.0))),
            // A pyramid: A h / 3; four triangles of base 4 and slant height √(9 + 4).
            answer: Answer::Solid { volume: 16.0, area: Some(16.0 + 8.0 * 13f64.sqrt()) },
        },
        Case {
            op: "split",
            name: "a 10 cube split at z 3, the piece below",
            build: || piece(split(cube(0.0, 10.0), v(0.0, 0.0, 3.0), v(0.0, 0.0, 1.0)), 0),
            answer: Answer::Solid { volume: 300.0, area: Some(200.0 + 4.0 * 30.0) },
        },
        Case {
            op: "split",
            name: "a 10 cube split at z 3, the piece above",
            build: || piece(split(cube(0.0, 10.0), v(0.0, 0.0, 3.0), v(0.0, 0.0, 1.0)), 1),
            answer: Answer::Solid { volume: 700.0, area: Some(200.0 + 4.0 * 70.0) },
        },
        Case {
            op: "split",
            name: "a 10 cube split corner to corner, the back half",
            build: || piece(split(cube(0.0, 10.0), v(0.0, 0.0, 0.0), v(1.0, -1.0, 0.0)), 0),
            // Two right triangles of 50, two sides of 100, the diagonal 10 x 10√2.
            answer: Answer::Solid { volume: 500.0, area: Some(300.0 + 100.0 * 2f64.sqrt()) },
        },
        Case {
            op: "split",
            name: "an r 2 cylinder 5 long halved along its axis, the back half",
            build: || piece(split(solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 2.0, 5.0), v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)), 0),
            // Half the wall (π r h), two half discs, the 4 x 5 section.
            answer: Answer::Solid { volume: 10.0 * PI, area: Some(10.0 * PI + 4.0 * PI + 20.0) },
        },
        Case {
            op: "split",
            name: "a plane that misses a 10 cube (the whole cube back)",
            build: || piece(split(cube(0.0, 10.0), v(0.0, 0.0, 20.0), v(0.0, 0.0, 1.0)), 0),
            answer: Answer::Solid { volume: 1000.0, area: Some(600.0) },
        },
        Case { op: "trim", name: "a 10 square sheet trimmed at x 3, the right kept", build: || trimmed(false), answer: Answer::Sheet { area: 70.0 } },
        Case {
            op: "trim",
            name: "a 10 square sheet with an r 2 hole trimmed through it, the right kept",
            build: || trimmed(true),
            answer: Answer::Sheet { area: 50.0 - 2.0 * PI },
        },
    ]
}

/// One case on one kernel.
#[derive(serde::Serialize)]
struct Run {
    /// pass, fail, unsupported.
    verdict: &'static str,
    why: String,
    volume_error: Option<f64>,
    area_error: Option<f64>,
    worst_tolerance: Option<f64>,
    ms: f64,
}

fn relative(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs().max(1.0)
}

fn worst_tolerance(b: &Body) -> f64 {
    let r = b.record();
    r.edges.iter().map(|e| e.tolerance).chain(r.vertices.iter().map(|v| v.tolerance)).fold(r.tolerance, f64::max)
}

fn run(case: &Case, kernel: Kernel) -> Run {
    foreign::set_kernel(kernel);
    let before = foreign::fell_back();
    let t = Instant::now();
    let got = (case.build)();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let unsupported = kernel == Kernel::Occt && foreign::fell_back() > before;
    let mut out = Run { verdict: "fail", why: String::new(), volume_error: None, area_error: None, worst_tolerance: None, ms };
    match (&case.answer, got) {
        (Answer::Solid { volume, area }, Ok(Some(body))) => {
            out.worst_tolerance = Some(worst_tolerance(&body));
            match solidcraft_kernel::measure(&body) {
                Ok(m) => {
                    let ve = relative(m.volume, *volume);
                    let ae = area.map(|a| relative(m.area, a));
                    out.volume_error = Some(ve);
                    out.area_error = ae;
                    let check = solidcraft_kernel::failing_check(&body);
                    if ve > EXACT || ae.is_some_and(|e| e > EXACT) {
                        out.why = format!("volume {} (want {volume}), area {}", m.volume, m.area);
                    } else if !matches!(check, Ok(None)) {
                        out.why = format!("the shared check fails: {check:?}");
                    } else {
                        out.verdict = "pass";
                    }
                }
                Err(e) => out.why = format!("not measured: {e}"),
            }
        }
        (Answer::Sheet { area }, Ok(Some(body))) => {
            out.worst_tolerance = Some(worst_tolerance(&body));
            match solidcraft_kernel::measure(&body) {
                Ok(m) => {
                    let ae = relative(m.area, *area);
                    out.area_error = Some(ae);
                    if !body.is_surface() {
                        out.why = "not a sheet".into();
                    } else if ae > EXACT {
                        out.why = format!("area {} (want {area})", m.area);
                    } else {
                        out.verdict = "pass";
                    }
                }
                Err(e) => out.why = format!("not measured: {e}"),
            }
        }
        (Answer::Solid { .. } | Answer::Sheet { .. }, Ok(None)) => out.why = "empty".into(),
        (Answer::Empty, Ok(None)) | (Answer::Refused, Err(_)) => out.verdict = "pass",
        (Answer::Empty | Answer::Refused, Ok(Some(_))) => out.why = "made a body".into(),
        (Answer::Refused, Ok(None)) => out.why = "empty, not refused".into(),
        (_, Err(e)) => out.why = e.to_string(),
    }
    if unsupported {
        out.verdict = "unsupported";
    }
    out
}

fn main() {
    let json = std::env::args().skip_while(|a| a != "--json").nth(1);
    let mut rows = Vec::new();
    let mut table = String::from(
        "| op | case | solidcore | occt | volume error (sc / occt) | area error (sc / occt) | worst tolerance (sc / occt) | ms (sc / occt) |\n|---|---|---|---|---|---|---|---|\n",
    );
    let e = |x: Option<f64>| x.map_or("-".to_string(), |x| format!("{x:.1e}"));
    for case in cases() {
        let sc = run(&case, Kernel::Solidcore);
        let oc = run(&case, Kernel::Occt);
        table.push_str(&format!(
            "| {} | {} | {}{} | {}{} | {} / {} | {} / {} | {} / {} | {:.1} / {:.1} |\n",
            case.op,
            case.name,
            sc.verdict,
            if sc.why.is_empty() { String::new() } else { format!(" ({})", sc.why) },
            oc.verdict,
            if oc.why.is_empty() { String::new() } else { format!(" ({})", oc.why) },
            e(sc.volume_error),
            e(oc.volume_error),
            e(sc.area_error),
            e(oc.area_error),
            e(sc.worst_tolerance),
            e(oc.worst_tolerance),
            sc.ms,
            oc.ms,
        ));
        rows.push(serde_json::json!({ "op": case.op, "case": case.name, "solidcore": sc, "occt": oc }));
    }
    let mut summary = String::from(
        "\n| op | cases | solidcore pass | occt pass / fail / unsupported | occt looser than solidcore (any / above 1e-12) |\n|---|---|---|---|---|\n",
    );
    for op in [
        "box",
        "cylinder",
        "sphere",
        "torus",
        "extrude",
        "revolve",
        "boolean",
        "fillet",
        "chamfer",
        "shell",
        "thicken",
        "offset faces",
        "draft",
        "loft",
        "split",
        "trim",
    ] {
        let of: Vec<&serde_json::Value> = rows.iter().filter(|r| r["op"] == op).collect();
        let count = |k: &str, v: &str| of.iter().filter(|r| r[k]["verdict"] == v).count();
        // Looser: larger than solidcore's beyond the last bits (1e-15), and also above 1e-12.
        let looser = |floor: f64| {
            of.iter()
                .filter(|r| {
                    ["volume_error", "area_error", "worst_tolerance"].iter().any(|m| match (r["solidcore"][m].as_f64(), r["occt"][m].as_f64()) {
                        (Some(s), Some(o)) => o > s + 1e-15 && o > floor,
                        _ => false,
                    })
                })
                .count()
        };
        summary.push_str(&format!(
            "| {op} | {} | {} | {} / {} / {} | {} / {} |\n",
            of.len(),
            count("solidcore", "pass"),
            count("occt", "pass"),
            count("occt", "fail"),
            count("occt", "unsupported"),
            looser(0.0),
            looser(1e-12)
        ));
    }
    println!("{table}{summary}");
    println!("occt: {}", foreign::occt_version());
    if let Some(path) = json {
        let text = serde_json::to_string_pretty(&rows).unwrap_or_default();
        if let Err(e) = std::fs::write(&path, text) {
            println!("could not write {path}: {e}");
        }
    }
}
