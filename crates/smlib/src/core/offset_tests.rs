//! Shells, face offsets, thickening, sheet offsets and drafts on solidcore, against closed forms:
//! every result exports to a record that validates.
//!
//! The bodies are solidcore's own boxes, cylinders and linear sweeps (and booleans of them).

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::mpsc;
use std::time::Duration;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;

use super::offset::{draft, mark_moved, moved_faces, offset_faces, offset_sheet, offset_solid, refused, shell, thicken};
use super::{Body, area, bounds, counts, make_box, make_cylinder, make_sphere, run, to_arrays, volume};
use crate::export::native;
use crate::{BooleanOp, Error};

/// A case longer than this is a hang.
const TIMEOUT: Duration = Duration::from_secs(300);

/// Stress corpus S0790 (#18802): the rim hole leaves a 0.00002 mm web at the
/// frustum's top. Inward shelling preserves that web. Moving the drill to x = 8.8
/// makes its rim touch the top boundary at (10, 0, 50), which keeps the diagnosed
/// outline-touch refusal and its nested kernel cause (#18568).
#[test]
fn shell_of_a_drilled_frustum_keeps_its_failure_reason() {
    let shelled = |x| {
        timed("S0790", move || {
            let cone = super::make_cone([0.0; 3], 25.0, 10.0, 50.0).unwrap();
            let rim_hole = make_cylinder([x, 0.0, 40.0], 1.2, 10.0).unwrap();
            let centre_hole = make_cylinder([0.0, 0.0, 42.0], 1.0, 8.0).unwrap();
            let drilled = super::boolean::boolean(&cone, &rim_hole, BooleanOp::Subtract).unwrap().unwrap();
            let drilled = super::boolean::boolean(&drilled, &centre_hole, BooleanOp::Subtract).unwrap().unwrap();
            shell(&drilled, &[], TOL, -1.2)
        })
    };

    let body = shelled(8.79998).expect("the positive-width web survives shelling");
    let check = super::check(&body, crate::check::CheckLevel::All).unwrap();
    assert!(check.is_valid(), "{check}");
    assert_eq!(super::solid_count(&body).unwrap(), 1);
    let (record, _) = native(&to_arrays(&body).unwrap()).unwrap();
    solidcore_ops::record_check::validate(&record).unwrap();
    let top: Vec<_> = record
        .faces
        .iter()
        .filter(|face| match &face.surface {
            solidcore_record::brep::Surface::Plane { origin, .. } | solidcore_record::brep::Surface::PlaneThrough { origin, .. } => {
                (origin[2] - 50.0).abs() < 1e-8
            }
            _ => false,
        })
        .collect();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].loops.len(), 3, "one outer boundary and two separate blind holes");
    // The outer drilled volume is 16227.6*pi. The inward cavity has radius
    // R(z) = 25 - 0.3*z - 1.2*sqrt(1.09), for z in [1.2, 48.8]. Its excluded
    // central hole has radius 2.2 above z=40.8. Its eccentric hole has radius
    // 2.4 above z=38.8, clipped to that circular section. Integrating these
    // independent sections gives this material volume.
    let want = 9243.7803299802;
    let got = volume(&body, 1e-8).unwrap();
    assert!(rel(got, want) < 1e-9, "volume {got}, independently derived {want}");

    let error = shelled(8.8).expect_err("the hole actually touches the top outline");
    let words = error.to_string();
    let Error::OutlineTouches { at, .. } = &error else { panic!("{words}") };
    assert!((at[0] - 10.0).abs() < 1e-4 && at[1].abs() < 1e-4 && (at[2] - 50.0).abs() < 1e-4, "at {at:?}");
    assert!(!words.contains("upstream status"), "{words}");
    assert!(words.contains("an offset edge could not be split where a loop gap or a step corner needs it"), "{words}");
    eprintln!("{words}");
}

#[test]
fn typed_offset_refusals_keep_their_nested_kernel_cause() {
    use solidcore_ops::{Error as Ops, OffsetRefusal};

    let refusal = Ops::OffsetRefused { face: None, why: OffsetRefusal::ExactCurveUncut };
    let diagnostic = "offset::trim: exact_curve_uncut; caused by intersect::clip: the exact trim does not cross the face boundary".to_owned();
    let error = refused(refusal.clone(), diagnostic.clone());
    let words = error.to_string();
    assert!(words.contains(&refusal.to_string()), "{words}");
    assert!(words.contains(&diagnostic), "{words}");
    assert_eq!(error, Error::RefusedWithDiagnostic { refusal, diagnostic });
}

#[test]
fn offset_folds_keep_the_largest_wall() {
    let refusal = solidcore_ops::Error::WallFolds { largest: Some(1.0) };
    assert_eq!(refused(refusal, "offset::face: a folded wall".into()), Error::WallFolds { largest: Some(1.0) });
}

/// How near a pick point must be.
const TOL: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Box([f64; 3], [f64; 3]),
    Cylinder([f64; 3], f64, f64),
    Sphere([f64; 3], f64),
    /// A frustum about +Z: base centre, base radius, top radius, height.
    Frustum([f64; 3], f64, f64, f64),
    /// A sheet: the line from the first point to the second swept along `dir` by `dist`.
    LineSheet([f64; 3], [f64; 3], [f64; 3], f64),
    /// A sheet: the arc about +Z through `center` of `radius` from 0 to `degrees`, swept along
    /// +Z by `height`.
    ArcSheet([f64; 3], f64, f64, f64),
    /// Rig case shell/4711: a 60 x 40 x 10 block less an r3 through hole at (22.5, 7) and a
    /// 10 x 10 notch on the front edge from x = 25 to 35.
    Notched,
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

impl Shape {
    fn core(self) -> Body {
        match self {
            Shape::Box(o, s) => make_box(o, s).unwrap(),
            Shape::Cylinder(o, r, h) => make_cylinder(o, r, h).unwrap(),
            Shape::Sphere(o, r) => make_sphere(o, r).unwrap(),
            Shape::Frustum(o, a, b, h) => super::make_cone(o, a, b, h).unwrap(),
            Shape::LineSheet(a, b, dir, dist) => run("sheet", |k| {
                let c = Curve::from(create_line_segment(v3(a), v3(b))?);
                create_linear_sweep(k, &[Some(&c)], v3(dir), dist, false)
            })
            .map(Body::from_kernel)
            .unwrap(),
            Shape::ArcSheet(o, r, degrees, h) => run("sheet", |k| {
                let frame = Axis2Placement::new(v3(o), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
                let c = Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&Extent1d::from_min_max(0.0, degrees)), None)?);
                create_linear_sweep(k, &[Some(&c)], Vec3::new(0.0, 0.0, 1.0), h, false)
            })
            .map(Body::from_kernel)
            .unwrap(),
            Shape::Notched => {
                let block = make_box([0.0; 3], [60.0, 40.0, 10.0]).unwrap();
                let pin = make_cylinder([22.5, 7.0, -5.0], 3.0, 20.0).unwrap();
                let notch = make_box([25.0, -5.0, -5.0], [10.0, 10.0, 20.0]).unwrap();
                let a = super::boolean::boolean(&block, &pin, BooleanOp::Subtract).unwrap().unwrap();
                super::boolean::boolean(&a, &notch, BooleanOp::Subtract).unwrap().unwrap()
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Op {
    /// `Brep::shell`: open faces, thickness.
    Shell(Shape, Vec<[f64; 3]>, f64),
    /// `Brep::offset_faces`: faces, distance.
    OffsetFaces(Shape, Vec<[f64; 3]>, f64),
    /// `Brep::thicken`: thickness, symmetric.
    Thicken(Shape, f64, bool),
    /// `Brep::offset_sheet`: distance.
    OffsetSheet(Shape, f64),
    /// `Brep::offset_solid`: distance, round.
    OffsetSolid(Shape, f64, bool),
    /// `Brep::draft`: faces, neutral origin, neutral normal, pull, angle.
    Draft(Shape, Vec<[f64; 3]>, [f64; 3], [f64; 3], [f64; 3], f64),
}

/// What a case must give.
#[derive(Clone, Debug)]
enum Expect {
    /// A solid of this volume (relative 1e-9).
    Volume(f64),
    /// A solid of this volume whose bounds along z are these.
    Slab(f64, f64, f64),
    /// A solid of this volume (relative 1e-9) whose bounds are these.
    Solid(f64, [f64; 3], [f64; 3]),
    /// A sheet of this area (relative 1e-9) whose bounds are these.
    Sheet(f64, [f64; 3], [f64; 3]),
    /// This error.
    Fails(Error),
}

struct Case {
    op: Op,
    expect: Expect,
}

/// A 30 x 20 x 10 block from the origin.
const BLOCK: Shape = Shape::Box([0.0; 3], [30.0, 20.0, 10.0]);
const ABC: f64 = 6000.0;
/// A point on its top face.
const TOP: [f64; 3] = [15.0, 10.0, 10.0];
/// A point on its front face (y = 0).
const FRONT: [f64; 3] = [15.0, 0.0, 5.0];
/// The z axis: a neutral plane's normal and a pull.
const Z: [f64; 3] = [0.0, 0.0, 1.0];
/// A 30 x 20 sheet in z = 0: the x axis from 0 to 30 swept 20 along y.
const PLATE: Shape = Shape::LineSheet([0.0; 3], [30.0, 0.0, 0.0], [0.0, 1.0, 0.0], 20.0);
/// A sphere of radius 10 about the origin.
const BALL: Shape = Shape::Sphere([0.0; 3], 10.0);
/// A quarter of a cylinder of radius 5 about the z axis, 10 high.
const QUARTER: Shape = Shape::ArcSheet([0.0; 3], 5.0, 90.0, 10.0);

fn case(name: &str) -> Case {
    let c = |op, expect| Case { op, expect };
    match name {
        // abc - (a - 2t)(b - 2t)(c - t), t = 2.
        "shell_box_open_top" => c(Op::Shell(BLOCK, vec![TOP], -2.0), Expect::Volume(ABC - 26.0 * 16.0 * 8.0)),
        // A void: abc - (a - 2t)(b - 2t)(c - 2t).
        "shell_box_closed" => c(Op::Shell(BLOCK, vec![], -2.0), Expect::Volume(ABC - 26.0 * 16.0 * 6.0)),
        // Closed and outward, the convex edges extended: (a + 2t)(b + 2t)(c + 2t) - abc, t = 1.
        "shell_box_closed_outward" => c(Op::Shell(BLOCK, vec![], 1.0), Expect::Volume(32.0 * 22.0 * 12.0 - ABC)),
        // Outward, the convex edges extended: (a + 2t)(b + 2t)(c + t) - abc, t = 1.
        "shell_box_outward" => c(Op::Shell(BLOCK, vec![TOP], 1.0), Expect::Volume(32.0 * 22.0 * 11.0 - ABC)),
        // r = 5, h = 10, open at the top, t = 1: π r² h - π (r - t)² (h - t).
        "shell_cylinder_open_top" => {
            c(Op::Shell(Shape::Cylinder([0.0; 3], 5.0, 10.0), vec![[0.0, 2.0, 10.0]], -1.0), Expect::Volume(PI * 25.0 * 10.0 - PI * 16.0 * 9.0))
        }
        // A wall of 15 inside a block 20 wide leaves no cavity.
        "shell_too_thick" => c(Op::Shell(BLOCK, vec![TOP], -15.0), Expect::Fails(Error::NoRoom)),
        // The hole's offset (r 3 + 2) is tangent to the front's (y = 2): the walls meet along a
        // line, which bounds no solid.
        "shell_walls_touch" => c(Op::Shell(Shape::Notched, vec![[50.0, 30.0, 0.0]], -2.0), Expect::Fails(Error::WallsTouch)),
        "shell_pick_near_nothing" => c(Op::Shell(BLOCK, vec![TOP, [100.0; 3]], -2.0), Expect::Fails(Error::NoMatch { point: 1 })),
        // The top front edge's midpoint is as near the top as the front.
        "shell_pick_ambiguous" => c(Op::Shell(BLOCK, vec![[15.0, 0.0, 10.0]], -2.0), Expect::Fails(Error::Ambiguous { point: 0 })),
        // A plate of 30 x 20 thickened 2: 1200, 2 thick along z; symmetric, from -1 to 1.
        "thicken_plate" => c(Op::Thicken(PLATE, 2.0, false), Expect::Slab(1200.0, 0.0, 2.0)),
        "thicken_plate_symmetric" => c(Op::Thicken(PLATE, 2.0, true), Expect::Slab(1200.0, -1.0, 1.0)),
        "thicken_a_solid" => c(Op::Thicken(BLOCK, 2.0, false), Expect::Fails(Error::Argument)),
        // The plate moved 3 along its normal (+z: the x axis swept along y): still 600, at z = 3.
        "offset_sheet_plate" => c(Op::OffsetSheet(PLATE, 3.0), Expect::Sheet(600.0, [0.0, 0.0, 3.0], [30.0, 20.0, 3.0])),
        // A sphere's one face closes on itself (tangency-labs/smlib fix #17): r = 10 moved out 2 is
        // the ball of radius 12, (4/3) π 12³; moved in 10, to its centre, nothing is left.
        "offset_sheet_a_sphere" => c(Op::OffsetSheet(BALL, 2.0), Expect::Volume(4.0 / 3.0 * PI * 12.0f64.powi(3))),
        "offset_sheet_a_sphere_to_its_centre" => c(Op::OffsetSheet(BALL, -10.0), Expect::Fails(Error::Empty)),
        // The quarter cylinder moved 2 along its normal (outward: the arc's tangent swept along
        // z): radius 7, area (π/2) 7 · 10.
        "offset_sheet_quarter_cylinder" => c(Op::OffsetSheet(QUARTER, 2.0), Expect::Sheet(PI / 2.0 * 7.0 * 10.0, [0.0, 0.0, 0.0], [7.0, 7.0, 10.0])),
        // A closed sheet offsets as an open one (tangency-labs/smlib fix #17, develop 9d0fb564):
        // each face moved out 1 along its normal, the convex edges extended, gives the block
        // (a + 2d)(b + 2d)(c + 2d) of six planes, from -d to (a, b, c) + d.
        "offset_sheet_a_solid" => c(Op::OffsetSheet(BLOCK, 1.0), Expect::Solid(32.0 * 22.0 * 12.0, [-1.0, -1.0, -1.0], [31.0, 21.0, 11.0])),
        // A closed solid grown 1, the convex edges extended: the block (a + 2d)(b + 2d)(c + 2d),
        // from -d to (a, b, c) + d.
        "offset_solid_sharp" => c(Op::OffsetSolid(BLOCK, 1.0, false), Expect::Solid(32.0 * 22.0 * 12.0, [-1.0, -1.0, -1.0], [31.0, 21.0, 11.0])),
        // Shrunk 1, sharp: (a - 2d)(b - 2d)(c - 2d), from d to (a, b, c) - d.
        "offset_solid_shrunk" => c(Op::OffsetSolid(BLOCK, -1.0, false), Expect::Solid(28.0 * 18.0 * 8.0, [1.0, 1.0, 1.0], [29.0, 19.0, 9.0])),
        // Grown 1 with the convex edges rounded: every point within d of the block (Steiner's
        // formula for a box): abc + 2(ab + bc + ca) d + π (a + b + c) d² + (4/3) π d³; the six
        // faces moved out, a quarter cylinder along each edge, an eighth of the ball at each
        // corner. Its bounds are the sharp offset's.
        "offset_solid_round" => c(
            Op::OffsetSolid(BLOCK, 1.0, true),
            Expect::Solid(ABC + 2.0 * 1100.0 + PI * 60.0 + 4.0 / 3.0 * PI, [-1.0, -1.0, -1.0], [31.0, 21.0, 11.0]),
        ),
        // A cylinder r 5, h 10 grown d = 1, rounded: the side moved out, π (r + d)² h; the caps
        // moved out, 2 π r² d; at each rim a quarter disc of radius d (area π d² / 4, centroid
        // 4d / (3π) out from the rim) turned about the axis, 2 π (r + 4d / (3π)) π d² / 4 by Pappus.
        "offset_solid_round_cylinder" => {
            let (r, h, d) = (5.0, 10.0, 1.0);
            let rim = 2.0 * PI * (r + 4.0 * d / (3.0 * PI)) * PI * d * d / 4.0;
            c(
                Op::OffsetSolid(Shape::Cylinder([0.0; 3], r, h), d, true),
                Expect::Solid(PI * (r + d) * (r + d) * h + 2.0 * PI * r * r * d + 2.0 * rim, [-6.0, -6.0, -1.0], [6.0, 6.0, 11.0]),
            )
        }
        // A sheet is not a closed solid.
        "offset_solid_of_a_sheet" => c(Op::OffsetSolid(PLATE, 1.0, false), Expect::Fails(Error::Argument)),
        // The top moved out 2: abc + d·ab.
        "offset_faces_top_out" => c(Op::OffsetFaces(BLOCK, vec![TOP], 2.0), Expect::Volume(ABC + 2.0 * 600.0)),
        // The top moved in 10, as deep as the block: nothing left behind it.
        "offset_faces_through" => c(Op::OffsetFaces(BLOCK, vec![TOP], -10.0), Expect::Fails(Error::NoRoom)),
        // The four sides drafted θ = 5° about the bottom (z = 0), pulled along +z, leaning in:
        // the section at height z is (a - 2 z tanθ)(b - 2 z tanθ), so
        // V = abh - (a + b) h² tanθ + (4/3) h³ tan²θ.
        "draft_box_sides" => {
            let s = (5.0f64).to_radians().tan();
            let (a, b, h) = (30.0, 20.0, 10.0);
            let sides = vec![[15.0, 0.0, 5.0], [30.0, 10.0, 5.0], [15.0, 20.0, 5.0], [0.0, 10.0, 5.0]];
            c(
                Op::Draft(BLOCK, sides, [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 5f64.to_radians()),
                Expect::Volume(a * b * h - (a + b) * h * h * s + 4.0 / 3.0 * h * h * h * s * s),
            )
        }
        // The front (y = 0) alone drafted θ = 5° about the bottom: a wedge of a h² tanθ / 2
        // comes off along its length.
        "draft_one_side" => {
            let s = (5.0f64).to_radians().tan();
            c(Op::Draft(BLOCK, vec![FRONT], [0.0; 3], Z, Z, 5f64.to_radians()), Expect::Volume(ABC - 30.0 * 100.0 * s / 2.0))
        }
        // A negative angle leans it out: the same wedge goes on.
        "draft_one_side_out" => {
            let s = (5.0f64).to_radians().tan();
            c(Op::Draft(BLOCK, vec![FRONT], [0.0; 3], Z, Z, -5f64.to_radians()), Expect::Volume(ABC + 30.0 * 100.0 * s / 2.0))
        }
        // Hinged at the top (z = 10) and leaning in going up, the face leans out below the
        // hinge: the wedge goes on.
        "draft_one_side_hinged_at_the_top" => {
            let s = (5.0f64).to_radians().tan();
            c(Op::Draft(BLOCK, vec![FRONT], [0.0, 0.0, 10.0], Z, Z, 5f64.to_radians()), Expect::Volume(ABC + 30.0 * 100.0 * s / 2.0))
        }
        // A cylinder along the pull drafts to a cone (tangency-labs/smlib fix #27, develop
        // 9d0fb564): R = 5, h = 10, θ = 0.1 about the bottom, leaning in: a frustum of radii R
        // and r = R - h tanθ (a cone and its two caps): V = π h (R² + R r + r²) / 3.
        "draft_a_curved_face" => {
            let (big, h) = (5.0, 10.0);
            let small = big - h * 0.1f64.tan();
            c(
                Op::Draft(Shape::Cylinder([0.0; 3], big, h), vec![[5.0, 0.0, 5.0]], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 0.1),
                Expect::Volume(PI * h * (big * big + big * small + small * small) / 3.0),
            )
        }
        // A cone r 10 at its base to 8 at its top, 20 tall, drafted 5° about its base turns its
        // generators 5° more (fork d51a723): r 10 + 20 tan(atan(-0.1) - 5°) at the top.
        "draft_a_cone_about_the_pull" => {
            let (big, h, a) = (10.0, 20.0, 5f64.to_radians());
            let small = big + h * ((-0.1f64).atan() - a).tan();
            c(
                Op::Draft(Shape::Frustum([0.0; 3], big, 8.0, h), vec![[0.0, 9.0, 10.0]], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], a),
                Expect::Volume(PI * h * (big * big + big * small + small * small) / 3.0),
            )
        }
        // The top is square to the pull, not along it.
        "draft_a_face_across_the_pull" => c(
            Op::Draft(BLOCK, vec![TOP], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 0.1),
            Expect::Fails(draft_refused("a drafted plane is not parallel to the pull")),
        ),
        // A sphere is neither a plane nor a cone or cylinder (#18541).
        "draft_a_sphere" => c(
            Op::Draft(BALL, vec![[10.0, 0.0, 0.0]], [0.0; 3], Z, Z, 0.1),
            Expect::Fails(draft_refused("a drafted face is neither a plane nor a cone or cylinder")),
        ),
        // The cylinder's axis is z, the pull x.
        "draft_a_cylinder_across_its_axis" => c(
            Op::Draft(Shape::Cylinder([0.0; 3], 5.0, 10.0), vec![[5.0, 0.0, 5.0]], [0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.1),
            Expect::Fails(draft_refused("a drafted cone or cylinder's axis is not along the pull")),
        ),
        // The pull runs along the cylinder's axis z, but the neutral plane x = 0 holds that axis.
        "draft_a_cylinder_about_a_plane_through_its_axis" => c(
            Op::Draft(Shape::Cylinder([0.0; 3], 5.0, 10.0), vec![[5.0, 0.0, 5.0]], [0.0; 3], [1.0, 0.0, 0.0], Z, 0.1),
            Expect::Fails(draft_refused("a drafted cone or cylinder's axis is parallel to the neutral plane")),
        ),
        // The neutral plane x = 10 misses the axis x = 0 but runs along it: the same refusal,
        // without claiming the axis lies in the plane.
        "draft_a_cylinder_about_a_plane_along_its_axis" => c(
            Op::Draft(Shape::Cylinder([0.0; 3], 5.0, 10.0), vec![[5.0, 0.0, 5.0]], [10.0, 0.0, 0.0], [1.0, 0.0, 0.0], Z, 0.1),
            Expect::Fails(draft_refused("a drafted cone or cylinder's axis is parallel to the neutral plane")),
        ),
        // The cone r 10 at its base to 8 at its top, 20 tall, drafted 0.5 rad about its base:
        // its top radius would be 10 + 20 tan(atan(-0.1) - 0.5) = -3.68, so it closes on the way up.
        "draft_a_cone_until_it_closes" => c(
            Op::Draft(Shape::Frustum([0.0; 3], 10.0, 8.0, 20.0), vec![[0.0, 9.0, 10.0]], [0.0; 3], Z, Z, 0.5),
            Expect::Fails(Error::Refused(solidcore_ops::Error::OffsetRefused { face: None, why: solidcore_ops::OffsetRefusal::DraftConeCloses })),
        ),
        // #22807: Ø20 x 100 cylinder drafted 6° about its base closes at z = 10 / tan(6°) ~ 95.14 < 100.
        "draft_a_cylinder_until_it_closes" => c(
            Op::Draft(Shape::Cylinder([0.0; 3], 10.0, 100.0), vec![[10.0, 0.0, 50.0]], [0.0; 3], Z, Z, 6f64.to_radians()),
            Expect::Fails(Error::Refused(solidcore_ops::Error::OffsetRefused { face: None, why: solidcore_ops::OffsetRefusal::DraftConeCloses })),
        ),
        // The same cone about the plane z = 150, past its apex at z = 100: its radius there,
        // 10 - 0.1 * 150 = -5, is not positive. The face itself does not close: the refusal names
        // the neutral plane.
        "draft_a_cone_about_a_plane_past_its_apex" => c(
            Op::Draft(Shape::Frustum([0.0; 3], 10.0, 8.0, 20.0), vec![[0.0, 9.0, 10.0]], [0.0, 0.0, 150.0], Z, Z, 0.1),
            Expect::Fails(Error::Refused(solidcore_ops::Error::OffsetRefused {
                face: None,
                why: solidcore_ops::OffsetRefusal::DraftNeutralPlanePastApex,
            })),
        ),
        _ => panic!("{name}"),
    }
}

/// The tolerance a case picks with: the ambiguous point lies on its two faces.
fn tolerance(name: &str) -> f64 {
    if name == "shell_pick_ambiguous" { 1.0 } else { TOL }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

#[derive(Debug, PartialEq)]
struct Measured {
    /// The volume of a solid, the area of a sheet.
    size: f64,
    bounds: ([f64; 3], [f64; 3]),
    counts: [usize; 3],
    kinds: BTreeMap<String, usize>,
}

fn kinds(rec: &solidcore_record::brep::Brep) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for f in &rec.faces {
        let v = serde_json::to_value(&f.surface).unwrap();
        *out.entry(v["kind"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
    }
    out
}

fn is_sheet(e: &Expect) -> bool {
    matches!(e, Expect::Sheet(..))
}

fn on_core(name: &str, c: &Case) -> crate::Result<Body> {
    let tol = tolerance(name);
    match &c.op {
        Op::Shell(s, points, t) => shell(&s.core(), points, tol, *t),
        Op::OffsetFaces(s, points, d) => offset_faces(&s.core(), points, tol, *d),
        Op::Thicken(s, t, sym) => thicken(&s.core(), *t, *sym),
        Op::OffsetSheet(s, d) => offset_sheet(&s.core(), *d),
        Op::OffsetSolid(s, d, round) => offset_solid(&s.core(), *d, *round),
        Op::Draft(s, points, o, n, pull, angle) => draft(&s.core(), points, tol, *o, *n, *pull, *angle),
    }
}

/// Runs `f` on its own thread: a hang fails the test instead of stopping the run.
fn timed<T: Send + 'static>(name: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(v) => v,
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(p) => std::panic::resume_unwind(p),
            Ok(()) => panic!("{name}: no result"),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("{name}: solidcore hung (no result after {TIMEOUT:?})"),
    }
}

type Got = std::result::Result<Measured, Error>;

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// Checks a kernel's result against the case's closed form; what it measured, or its error.
fn judge(name: &str, who: &str, expect: &Expect, got: Got) -> Got {
    match (expect, &got) {
        (Expect::Fails(e), Err(g)) => assert_eq!(g, e, "{name}: {who} failed otherwise"),
        (Expect::Fails(e), Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
        (_, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (Expect::Volume(v), Ok(m)) => assert!(rel(m.size, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.size),
        (Expect::Slab(v, lo, hi), Ok(m)) => {
            assert!(rel(m.size, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.size);
            let (a, b) = (m.bounds.0[2], m.bounds.1[2]);
            assert!((a - lo).abs() < 1e-6 && (b - hi).abs() < 1e-6, "{name}: {who} z from {a} to {b}, expected {lo} to {hi}");
        }
        (Expect::Solid(v, lo, hi), Ok(m)) => {
            assert!(rel(m.size, *v) < 1e-9, "{name}: {who} volume {} vs analytic {v}", m.size);
            assert!(near(m.bounds.0, *lo) && near(m.bounds.1, *hi), "{name}: {who} bounds {:?}, expected {lo:?} {hi:?}", m.bounds);
        }
        (Expect::Sheet(s, lo, hi), Ok(m)) => {
            assert!(rel(m.size, *s) < 1e-9, "{name}: {who} area {} vs analytic {s}", m.size);
            assert!(near(m.bounds.0, *lo) && near(m.bounds.1, *hi), "{name}: {who} bounds {:?}, expected {lo:?} {hi:?}", m.bounds);
        }
    }
    got
}

fn solidcore_case(name: &'static str) -> Got {
    timed(name, move || {
        let c = case(name);
        let sheet = is_sheet(&c.expect);
        let got = on_core(name, &c).map(|body| {
            let size = if sheet { area(&body, 1e-8) } else { volume(&body, 1e-8) }.unwrap_or_else(|e| panic!("{name}: size: {e}"));
            let n = counts(&body).unwrap();
            let (rec, _) = native(&to_arrays(&body).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
            solidcore_ops::record_check::validate(&rec).unwrap_or_else(|e| panic!("{name}: record: {e}"));
            let bx = bounds(&body).unwrap();
            Measured { size, bounds: bx, counts: n, kinds: kinds(&rec) }
        });
        judge(name, "solidcore", &c.expect, got)
    })
}

macro_rules! cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_closed_form() {
                let _ = super::solidcore_case(stringify!($m));
            }
        }
    )*};
}

cases!(
    shell_box_open_top,
    shell_box_closed,
    shell_box_closed_outward,
    shell_box_outward,
    shell_cylinder_open_top,
    shell_too_thick,
    shell_walls_touch,
    shell_pick_near_nothing,
    shell_pick_ambiguous,
    thicken_plate,
    thicken_plate_symmetric,
    thicken_a_solid,
    offset_sheet_plate,
    offset_sheet_quarter_cylinder,
    offset_sheet_a_solid,
    offset_sheet_a_sphere,
    offset_sheet_a_sphere_to_its_centre,
    offset_solid_sharp,
    offset_solid_shrunk,
    offset_solid_round,
    offset_solid_round_cylinder,
    offset_solid_of_a_sheet,
    offset_faces_top_out,
    offset_faces_through,
    draft_box_sides,
    draft_one_side,
    draft_one_side_out,
    draft_one_side_hinged_at_the_top,
    draft_a_curved_face,
    draft_a_cone_about_the_pull,
    draft_a_face_across_the_pull,
    draft_a_sphere,
    draft_a_cylinder_across_its_axis,
    draft_a_cylinder_about_a_plane_through_its_axis,
    draft_a_cylinder_about_a_plane_along_its_axis,
    draft_a_cone_until_it_closes,
    draft_a_cylinder_until_it_closes,
    draft_a_cone_about_a_plane_past_its_apex,
);

/// A draft the kernel refuses for what the face is, in solidcore-ops' words for the same refusal.
fn draft_refused(why: &'static str) -> Error {
    Error::Refused(solidcore_ops::Error::Unsupported(why))
}

/// The capi's argument checks, before any kernel call.
#[test]
fn hostile_arguments_are_refused() {
    let b = BLOCK.core();
    for t in [0.0, f64::NAN, -1e7, f64::INFINITY] {
        assert_eq!(shell(&b, &[TOP], TOL, t).err(), Some(Error::Argument), "thickness {t}");
        assert_eq!(thicken(&b, t, false).err(), Some(Error::Argument), "thickness {t}");
        assert_eq!(offset_sheet(&b, t).err(), Some(Error::Argument), "distance {t}");
        assert_eq!(offset_faces(&b, &[TOP], TOL, t).err(), Some(Error::Argument), "distance {t}");
    }
    assert_eq!(shell(&b, &[[f64::NAN; 3]], TOL, -1.0).err(), Some(Error::Argument));
    assert_eq!(shell(&b, &[TOP], 0.0, -1.0).err(), Some(Error::Argument));
    assert_eq!(offset_faces(&b, &[], TOL, 1.0).err(), Some(Error::Argument));
    let z = [0.0, 0.0, 1.0];
    assert_eq!(draft(&b, &[TOP], TOL, [0.0; 3], [0.0; 3], z, 0.1).err(), Some(Error::Argument), "no neutral normal");
    assert_eq!(draft(&b, &[TOP], TOL, [0.0; 3], z, z, 0.0).err(), Some(Error::Argument), "no angle");
    assert_eq!(draft(&b, &[TOP], TOL, [0.0; 3], z, z, PI / 2.0).err(), Some(Error::Argument), "a right angle");
    assert_eq!(draft(&b, &[TOP], TOL, [f64::NAN; 3], z, z, 0.1).err(), Some(Error::Argument));
}

/// The body offset is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let r = timed("operand", || {
        let b = BLOCK.core();
        let _ = shell(&b, &[TOP], TOL, -2.0);
        (counts(&b).unwrap(), volume(&b, 1e-8).unwrap())
    });
    assert_eq!(r.0, [6, 12, 8]);
    assert!(rel(r.1, ABC) < 1e-12, "{}", r.1);
}

/// The distance from `p` to each face of `b` (`SmApiFaceClosestPoint`), under `identity`.
fn face_distances(
    k: &mut solidcore::kernel::Kernel,
    b: &mut solidcore::topology::Brep,
    identity: &solidcore::query::brep::BrepIdentity<'_>,
    p: Vec3,
) -> solidcore::Result<Vec<(f64, Vec3)>> {
    use solidcore::geom::object::Shape as Sh;
    use solidcore::query::api_queries::face_closest_point_in_shape;
    use solidcore::query::brep::LiveBrepObjects;
    use solidcore::topology::BrepId;
    let one = BrepId::from_index(0);
    let mut out = Vec::new();
    for f in solidcore::topology::brep::p1::faces(b, None)? {
        let shape = Sh::new(k, &(one, &*b), &[], &[(one, f)], &[], &[])?;
        let objects = LiveBrepObjects::new(b, identity, None)?;
        let c = face_closest_point_in_shape(k, &shape, &objects, (one, f), p)?;
        out.push((c.distance, c.point));
    }
    Ok(out)
}

/// The closed form for (15, 10, 20) above the 30 x 20 x 10 box at the origin, face by face of
/// `create_box`: bottom 20, top 10, the x sides sqrt(15^2 + 10^2), the y sides sqrt(10^2 + 10^2).
fn box_face_distances_are_exact(what: &str, d: &[(f64, Vec3)]) {
    let want = [20.0, 10.0, 325f64.sqrt(), 325f64.sqrt(), 200f64.sqrt(), 200f64.sqrt()];
    assert_eq!(d.len(), want.len(), "{what}: faces");
    for (i, (&(got, _), want)) in d.iter().zip(want).enumerate() {
        assert!((got - want).abs() < 1e-9, "{what}: face {i} at {got}, want {want}");
    }
    let top = d[1].1;
    assert!((top.x - 15.0).abs() < 1e-9 && (top.y - 10.0).abs() < 1e-9 && (top.z - 10.0).abs() < 1e-9, "{what}: top {top:?}");
}

/// Picking faces on a solidcore box: on the body itself, on the working copy every operation
/// makes (`brep_copy`), and by solidcore's recommended usage, each face's distance is the closed
/// form and the top face is picked.
#[test]
fn faces_are_picked_on_the_box_and_on_its_working_copy() {
    use solidcore::query::brep::{BrepIdentity, BrepQuery};
    let p = Vec3::new(15.0, 10.0, 20.0);
    let on_top = [[15.0, 10.0, 10.0]];
    run("pick", |k| {
        let mut b = solidcore::primitive::api_primitives::create_box(k, Vec3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0)?;
        let mut w = solidcore::topology::brep::p3::brep_copy(k, &b)?;
        let mut faces = Vec::new();
        for (what, x) in [("the body", &mut b), ("its copy", &mut w)] {
            let query = BrepQuery::of_brep(x);
            let identity = query.effective_identity();
            box_face_distances_are_exact(what, &face_distances(k, x, &identity, p)?);
            let picked = super::offset::pick_faces(k, x, &on_top, TOL)?;
            faces.push(picked.ok());
        }
        assert!(faces[0].as_ref().is_some_and(|f| f.len() == 1), "the body: {faces:?}");
        assert_eq!(faces[0], faces[1], "the body and its copy pick the same face");
        // `BrepIdentity::of_brep` honours a copy's owner keys since solidcore#1445: the body
        // and its copy both resolve.
        for (what, x) in [("of_brep on the body", &mut b), ("of_brep on its copy", &mut w)] {
            let proven = BrepIdentity::of_brep(x);
            box_face_distances_are_exact(what, &face_distances(k, x, &proven.identity(), p)?);
        }
        Ok(())
    })
    .unwrap();
}

/// A pick skips an entity whose query returns an SMLib status, as the capi skips any status
/// other than `SM_SUCCESS`; any other solidcore error fails the pick.
#[test]
fn a_solidcore_error_in_a_pick_is_an_error() {
    use super::answer;
    use super::blend::pick;
    use solidcore::SolidError;
    let all = [0usize, 1];
    let points = [[0.0; 3]];
    let status = pick(&all, &points, 1.0, 1e-6, |i, _| answer(if i == 0 { Err(SolidError::sm_err("q")) } else { Ok(0.5) }));
    assert_eq!(status.unwrap().unwrap().0, vec![1], "a status skips the entity");
    let other = pick(&all, &points, 1.0, 1e-6, |i, _| answer(if i == 0 { Err(SolidError::invalid_input("q", "owner")) } else { Ok(0.5) }));
    assert!(matches!(other, Err(SolidError::InvalidInput { .. })), "{other:?}");
    let unported = pick(&all, &points, 1.0, 1e-6, |i, _| answer(if i == 0 { Err(SolidError::unported("Sm::X", "u")) } else { Ok(0.5) }));
    assert!(matches!(unported, Err(SolidError::Unported { .. })), "{unported:?}");
}

/// The capi's `moved_faces` reading: no mark, no move (SMLIB_ERR_NO_EFFECT); a mark is read once,
/// and gone.
#[test]
fn a_draft_with_no_marked_face_left_has_no_effect() {
    let body = make_box([0.0; 3], [10.0; 3]).unwrap();
    super::with_body("marks", &body, |_, w| {
        assert!(!moved_faces(w)?, "no face marked");
        let f = w.face_list()[0];
        mark_moved(w, &[f])?;
        assert!(moved_faces(w)?, "a face marked");
        assert!(!moved_faces(w)?, "the mark removed");
        Ok(())
    })
    .unwrap();
}

/// A draft leaves no mark of its own on what it returns, on a plane or on a cylinder turned cone.
#[test]
fn a_draft_leaves_no_mark() {
    let z = [0.0, 0.0, 1.0];
    let bx = make_box([0.0; 3], [20.0, 30.0, 10.0]).unwrap();
    let plane = draft(&bx, &[[10.0, 0.0, 5.0]], TOL, [0.0; 3], z, z, 0.1).unwrap();
    let cyl = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let cone = draft(&cyl, &[[5.0, 0.0, 5.0]], TOL, [0.0; 3], z, z, 0.1).unwrap();
    for b in [plane, cone] {
        super::with_body("marks", &b, |_, w| {
            assert!(!moved_faces(w)?);
            Ok(())
        })
        .unwrap();
    }
}

/// A stepped block, 40 x 30 x 10 with its half x < 20 raised to 20 (rig case
/// shell/cov-replace-face-step-onto-the-higher-face, the fork's `offset_faces_close_a_step_wall`):
/// a top moved onto the other's plane closes the step wall between them to nothing, and the tops
/// are one face of a 6-faced box.
#[test]
fn offset_faces_close_a_step_wall() {
    use super::boolean::boolean;
    let stepped = boolean(&make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(), &make_box([0.0; 3], [20.0, 30.0, 20.0]).unwrap(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    assert_eq!(counts(&stepped).unwrap()[0], 8);
    for (at, d, v, a) in [([30.0, 15.0, 10.0], 10.0, 24000.0, 5200.0), ([10.0, 15.0, 20.0], -10.0, 12000.0, 3800.0)] {
        let r = offset_faces(&stepped, &[at], 0.1, d).unwrap();
        let (got, n, area) = (volume(&r, 1e-8).unwrap(), counts(&r).unwrap(), super::area(&r, 1e-8).unwrap());
        assert_eq!(n[0], 6, "d {d}");
        assert!(rel(got, v) < 1e-9 && rel(area, a) < 1e-9, "d {d}: V {got} A {area}");
    }
}

/// Rig case shell/4492's rounded box: four mitred rounds, r 3, come off a 40 x 30 x 10 box's top
/// together and follow it. From height 0 the outline is the 34 x 24 top moved out by the round's
/// w(z) = sqrt(9 - (z - axis)^2), or 3 below the axis. With the fork's values at +19, and bodies
/// whose rounds go all the way down from -7 and are cut short from -7.5 (solidcore#2027). At -9,
/// which solidcore refused before 9856ffee5 (solidcore#2020), it builds the closed form too.
#[test]
fn mitred_rounds_follow_a_moved_top() {
    use super::blend::{Corners, blend};
    use crate::Blend;
    let edges = [[20.0, 0.0, 10.0], [20.0, 30.0, 10.0], [0.0, 15.0, 10.0], [40.0, 15.0, 10.0]];
    let top = [20.0, 15.0, 10.0];
    let rounded = blend(&make_box([0.0; 3], [40.0, 30.0, 10.0]).unwrap(), Blend::Round, &edges, &[3.0; 4], TOL, &[], &[], Corners::Filled).unwrap();
    let analytic = |d: f64| {
        let (height, axis, n) = (10.0 + d, 7.0 + d, 20000);
        (0..n)
            .map(|i| {
                let z = height * (f64::from(i) + 0.5) / f64::from(n);
                let w = if z < axis { 3.0 } else { (9.0 - (z - axis) * (z - axis)).max(0.0).sqrt() };
                (34.0 + 2.0 * w) * (24.0 + 2.0 * w) * height / f64::from(n)
            })
            .sum::<f64>()
    };
    let moves = [
        (19.0, [10, 20, 12]),
        (1.0, [10, 20, 12]),
        (-1.0, [10, 20, 12]),
        (-6.9, [10, 20, 12]),
        (-7.0, [6, 12, 8]),
        (-7.5, [6, 12, 8]),
        (-8.0, [6, 12, 8]),
        (-9.0, [6, 12, 8]),
        (-9.5, [6, 12, 8]),
    ];
    for (d, n) in moves {
        let moved = offset_faces(&rounded, &[top], 0.1, d).unwrap();
        let v = volume(&moved, 1e-8).unwrap();
        assert_eq!(counts(&moved).unwrap(), n, "d {d}");
        assert!(rel(v, analytic(d)) < 1e-6, "d {d}: {v} vs {}", analytic(d));
    }
}

/// A 100 x 60 x 5 plate extruded from a profile whose corners are rounded r 2 (#17414): its long
/// side moved out between two rounds builds the 100 x (60 + d) plate with its corners still
/// rounded, (100 (60 + d) - 4 (4 - pi)) 5. Each side's plane spans its own face only, so the new
/// corners where the rounds come off lie past them.
#[test]
fn a_side_between_profile_rounds_moves() {
    use crate::sweeps::{Curve, DraftCorner};
    let (w, h, r) = (100.0, 60.0, 2.0);
    let arc = |c: [f64; 2], start: f64| Curve::Arc {
        center: [c[0], c[1], 0.0],
        axis: Z,
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start,
        end: start + PI / 2.0,
    };
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], a[1], 0.0], end: [b[0], b[1], 0.0] };
    let profile = [
        line([r, 0.0], [w - r, 0.0]),
        arc([w - r, r], 1.5 * PI),
        line([w, r], [w, h - r]),
        arc([w - r, h - r], 0.0),
        line([w - r, h], [r, h]),
        arc([r, h - r], 0.5 * PI),
        line([0.0, h - r], [0.0, r]),
        arc([r, r], PI),
    ];
    let plate = super::sweep::extrude(&profile, Z, 5.0, 0.0, DraftCorner::default(), true).unwrap();
    for d in [80.0, 1.0] {
        let moved = offset_faces(&plate, &[[55.0, 0.0, 2.5]], TOL, d).unwrap_or_else(|e| panic!("d {d}: {e}"));
        let (v, want) = (volume(&moved, 1e-8).unwrap(), (w * (h + d) - 4.0 * (4.0 - PI)) * 5.0);
        assert_eq!(counts(&moved).unwrap()[0], 10, "d {d}");
        assert!(rel(v, want) < 1e-9, "d {d}: {v} vs {want}");
    }
}

/// `moved_faces`: drafts so small they leave a plane or cylinder where it was still moved a face
/// (solidcore's mark stays on it), and an angle that brings a wall onto the plane of the wall above
/// it leaves the two faces apart: every draft here builds, none is "no effect".
#[test]
fn a_draft_moves_a_face() {
    use crate::sweeps::{Curve, DraftCorner};
    let z = [0.0, 0.0, 1.0];
    let plate = make_box([0.0; 3], [40.0, 40.0, 10.0]).unwrap();
    let rod = make_cylinder([20.0, 20.0, -5.0], 5.0, 30.0).unwrap();
    let holed = super::boolean::boolean(&plate, &rod, BooleanOp::Subtract).unwrap().unwrap();
    let pinned = super::boolean::boolean(&plate, &rod, BooleanOp::Union).unwrap().unwrap();
    let t = 5f64.to_radians().tan();
    // A wall x = 20 up to z = 10, then a wall leaning in 5 degrees: the lower wall turned 5
    // degrees about z = 10 lies on the upper one's plane.
    let p = [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 0.0, 10.0], [20.0 - 10.0 * t, 0.0, 20.0], [0.0, 0.0, 20.0]];
    let profile: Vec<Curve> = (0..5).map(|i| Curve::Line { start: p[i], end: p[(i + 1) % 5] }).collect();
    let leaning = super::sweep::extrude(&profile, [0.0, 1.0, 0.0], 10.0, 0.0, DraftCorner::default(), true).unwrap();
    let five = 5f64.to_radians();
    let cases: [(&str, &Body, [f64; 3], [f64; 3], f64); 7] = [
        ("tiny on a plane", &plate, [20.0, 0.0, 5.0], [0.0; 3], 1e-14),
        ("tiny on a hole", &holed, [25.0, 20.0, 5.0], [0.0, 0.0, 5.0], 1e-14),
        ("tiny on a pin", &pinned, [25.0, 20.0, 20.0], [0.0, 0.0, 5.0], -1e-14),
        ("small on a hole", &holed, [25.0, 20.0, 5.0], [0.0, 0.0, 5.0], 1e-6),
        ("onto the wall above", &leaning, [20.0, 5.0, 5.0], [0.0, 0.0, 10.0], five),
        ("away from the wall above", &leaning, [20.0, 5.0, 5.0], [0.0, 0.0, 10.0], -five),
        ("hinged far off", &plate, [20.0, 0.0, 5.0], [0.0, 0.0, 1000.0], five),
    ];
    for (name, b, at, origin, angle) in cases {
        let r = draft(b, &[at], TOL, origin, z, z, angle).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(volume(&r, 1e-8).unwrap() > 0.0, "{name}");
    }
}

/// The tip cut off a triangular prism, drafted about a plane 100 mm off so that the whole face goes
/// out past where its neighbours meet: its edges vanish, `SM_ERR_OFFSET_EDGE_VANISHES` in SMLib,
/// refused with that reason (#13989), not as not supported.
#[test]
fn a_draft_past_the_neighbours_meeting_is_refused_as_an_edge_that_vanishes() {
    use crate::sweeps::{Curve, DraftCorner};
    let p = [[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [11.0, 18.0, 0.0], [9.0, 18.0, 0.0]];
    let profile: Vec<Curve> = (0..4).map(|i| Curve::Line { start: p[i], end: p[(i + 1) % 4] }).collect();
    let prism = super::sweep::extrude(&profile, [0.0, 0.0, 1.0], 10.0, 0.0, DraftCorner::default(), true).unwrap();
    let (tip, z, origin, angle) = ([[10.0, 18.0, 5.0]], [0.0, 0.0, 1.0], [0.0, 0.0, -100.0], (-2f64).to_radians());
    let vanishes = solidcore_ops::Error::OffsetRefused { face: None, why: solidcore_ops::OffsetRefusal::EdgeVanishes };
    assert!(matches!(
        draft(&prism, &tip, TOL, origin, z, z, angle).err(),
        Some(Error::RefusedWithDiagnostic { refusal, .. }) if refusal == vanishes
    ));
}

/// Stress corpus S8188 (#20767): a cone r 2 at z = 0 to r 1 at z = 3, shelled 0.12 in to a closed
/// cavity, its side drafted 3 degrees about its base, leaning in going up, through the kernel
/// wrapper's draft. The kernel's test (solidcore-ops `offset/tests.rs`,
/// `a_shelled_cones_side_drafted_into_its_cavity_is_refused_naming_the_largest_angle`) derives
/// the closed forms used here: the side reaches the cavity's rim at
/// atan((2 - rim) / 2.88) - atan(1/3) = 0.038994633885685748 rad (2.234 degrees), rim =
/// 2 - 0.04 sqrt(10) - 0.96, so 3 degrees is refused naming that angle, not as a body that fails
/// its check. With a 0.5 wall the rim is reached only at 10.118 degrees, and 3 degrees builds the
/// drafted cone less the cavity, 13.675559992581547.
#[test]
fn a_draft_into_a_shelled_cones_cavity_is_refused_naming_the_largest_angle() {
    const LARGEST: f64 = 0.038_994_633_885_685_75;
    const DRAFTED: f64 = 13.675_559_992_581_547;
    // 3 degrees, radians
    const THREE: f64 = 0.052_359_877_559_829_89;
    let z = [0.0, 0.0, 1.0];
    for wall in [0.12, 0.5] {
        let cone = super::make_cone([0.0; 3], 2.0, 1.0, 3.0).unwrap();
        let shelled = shell(&cone, &[], TOL, -wall).unwrap();
        let drafted = draft(&shelled, &[[0.0, -1.5, 1.5]], TOL, [0.0; 3], z, z, THREE);
        if wall == 0.12 {
            let Some(Error::RefusedWithDiagnostic { refusal, diagnostic }) = drafted.as_ref().err() else { panic!("{drafted:?}") };
            let solidcore_ops::Error::OffsetRefused { why: solidcore_ops::OffsetRefusal::DraftClosesWall { largest }, .. } = refusal else {
                panic!("{refusal}")
            };
            assert!((largest - LARGEST).abs() < 1e-12, "{largest}: {diagnostic}");
        } else {
            let got = volume(&drafted.unwrap(), 1e-8).unwrap();
            assert!(rel(got, DRAFTED) < 1e-9, "{got}");
        }
    }
}

/// A frustum r 100 at z = 0 to r 40 at z = 200 with six blind holes r 5 from z = 150 up through
/// the top on the circle r 32, its side drafted about its base to lean 18 degrees, through the
/// kernel wrapper's draft (the cone solidcore's draft gives, carrying its definition). The drafted
/// side, r(z) = 100 - z tan 18 degrees, is r 35.02 at the top and passes the holes' outer reach,
/// r 37, at z = 193.89, so every hole breaks out through it. The kernel's test (solidcore-ops
/// `offset/tests.rs`, `a_cones_side_drafted_through_blind_holes_near_its_top_breaks_them_out`)
/// derives the volume: the frustum less the holes, the discs' closed-form overlap integrated
/// over z, 3061182.2920648409. Here the side's cone comes back from the hole cuts with its
/// frame at the top of its axis, so the drafted cone's frame and stored heights differ from it.
#[test]
fn a_cones_side_drafted_through_blind_holes_near_its_top_breaks_them_out() {
    // 18 degrees less atan 0.3, radians
    const TURN: f64 = 0.022_702_470_881_112_213;
    const DRAFTED: f64 = 3_061_182.292_064_841;
    let z = [0.0, 0.0, 1.0];
    let s = 3.0_f64.sqrt();
    let mut body = super::make_cone([0.0; 3], 100.0, 40.0, 200.0).unwrap();
    for (x, y) in [(32.0, 0.0), (16.0, 16.0 * s), (-16.0, 16.0 * s), (-32.0, 0.0), (-16.0, -16.0 * s), (16.0, -16.0 * s)] {
        let hole = make_cylinder([x, y, 150.0], 5.0, 51.0).unwrap();
        body = super::boolean::boolean(&body, &hole, BooleanOp::Subtract).unwrap().unwrap();
    }
    let drafted = draft(&body, &[[70.0, 0.0, 100.0]], TOL, [0.0; 3], z, z, TURN).unwrap();
    let got = volume(&drafted, 1e-8).unwrap();
    assert!(rel(got, DRAFTED) < 1e-8, "{got}");
}

/// A flat L of two faces thickened by 2 with mitred walls: 2 x 10 x (20 + 22).
#[test]
fn an_l_sheet_thickens() {
    use crate::sweeps::{Curve, DraftCorner};
    let l = [Curve::Line { start: [0.0; 3], end: [20.0, 0.0, 0.0] }, Curve::Line { start: [20.0, 0.0, 0.0], end: [20.0, 20.0, 0.0] }];
    let sheet = super::sweep::extrude(&l, [0.0, 0.0, 1.0], 10.0, 0.0, DraftCorner::default(), false).unwrap();
    let core = thicken(&sheet, 2.0, false).unwrap();
    assert!(rel(volume(&core, 1e-8).unwrap(), 840.0) < 1e-9);
}

/// solidcore#2350's parabolic sheet: the quadratic Bézier (-10, 0), (0, 10), (10, 0) lofted 20
/// along z, a B-spline face whose radius of curvature at its apex is 10. Thickened 11 towards
/// its centre of curvature its offset folds there: solidcore's one construction refuses it
/// naming 10, the largest wall that works; 2 builds.
#[test]
fn a_wall_thicker_than_its_face_curves_is_refused_naming_the_largest() {
    use crate::sweeps::Curve;
    let parabola = |z: f64| Curve::Nurbs {
        degree: 2,
        points: vec![[-10.0, 0.0, z, 1.0], [0.0, 10.0, z, 1.0], [10.0, 0.0, z, 1.0]],
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    };
    let sheet = super::sweep::loft(&[vec![parabola(0.0)], vec![parabola(20.0)]], 1, false).unwrap();
    // Towards the centre is the side where an 11 wall folds; the other side takes any wall.
    let folds: Vec<String> = [11.0, -11.0].iter().filter_map(|&t| thicken(&sheet, t, false).err()).map(|e| e.to_string()).collect();
    let [why] = folds.as_slice() else { panic!("one side folds: {folds:?}") };
    let (words, largest) = why.split_once("; the largest wall that works is ").unwrap_or_else(|| panic!("{why}"));
    assert_eq!(words, "the wall is thicker than a face curves, so the face's offset folds");
    let largest: f64 = largest.parse().unwrap();
    assert!((largest - 10.0).abs() < 1e-9, "{largest}");
    assert!(thicken(&sheet, 2.0, false).is_ok() && thicken(&sheet, -2.0, false).is_ok());
}

/// A thread's flank moved along its normal (#15661, #15715, #16866): an M12 x 1.75 groove, its
/// walls joined across the turns as a modelled thread's are, cut one pitch deep into a 6 rod; the
/// lower flank, one B-spline face between the root and crest cylinders and the caps, moves out
/// and in. Its new surface is the offset of the flank's net, and how far the root and crest must
/// reach to meet it is proved: the move is no longer refused for want of that bound. The body
/// builds once the offset's loops close across the moved flank's ends (still refused there: "a
/// gap in its offset's loops cannot be bridged"); then it must pass the check and gain the
/// volume of the closed form below.
///
/// The flank is the screw surface `z = f(r) + c ψ` (`c = P / 2π`, `ψ` the turn from the
/// profile's plane), `f(r) = low − t (r − root)` with `t = tan 30°`. Its unit normal out of the
/// body is `(−f′ e_r − (c / r) e_ψ + e_z) / L`, `L = √(1 + t² + c² / r²)`, so the point at radius
/// `s` moves to radius `r′ = √(a² + b²)`, `a = s + d t / L`, `b = d c / (s L)`, turned by
/// `−atan2(b, a)` and raised by `d / L`: the offset is the screw surface
/// `z = g(r′) + c ψ` with `g(r′) = f(s) + d / L + c atan2(b, a)`. Between the caps, `H` apart,
/// each radius's strip `f ≤ z − c ψ ≤ g` turns through `H / c`, so the volume gained is
/// `(H / c) ∫ (g − f) r dr` from the root to the crest. Each flank, the old and the moved, lies
/// within the sweep's tolerance of its screw surface, so the volume is within twice that times
/// the flank's area `(H / c) ∫ L r dr` (the screw surface's area element is `L r dr dψ`).
#[test]
fn a_thread_flank_moves_beside_its_root_and_crest() {
    use super::sweep::screw_sweep;
    use crate::check::CheckLevel;
    use crate::sweeps::Curve as Profile;
    use std::f64::consts::TAU;
    let (pitch, crest, height) = (1.75, 6.0, 1.75);
    // H = √3 / 2 P; the root at 6 − 5H/8 and the profile out past the crest at 6 + P/20, the
    // flanks at 30° to the radius: each rises (out − root) tan 30° across them
    let tooth = 3f64.sqrt() / 2.0 * pitch;
    let (root, out, t) = (crest - 5.0 * tooth / 8.0, crest + pitch / 20.0, (PI / 6.0).tan());
    let flank = (out - root) * t;
    let (low, high) = (-pitch - pitch / 8.0, -pitch + pitch / 8.0);
    let corners = [[0.0, root, low], [0.0, out, low - flank], [0.0, out, high + flank], [0.0, root, high]];
    let groove: Vec<Profile> = (0..4).map(|i| Profile::Line { start: corners[i], end: corners[(i + 1) % 4] }).collect();
    // three turns from z below -0.9 carry the groove past both caps (its top above 2.6)
    let tolerance = pitch * 1e-4;
    let tool = screw_sweep(&groove, [0.0; 3], [0.0, 0.0, 1.0], pitch, 3.0, None, tolerance, true).unwrap();
    let rod = make_cylinder([0.0; 3], crest, height).unwrap();
    let thread = super::boolean::boolean(&rod, &tool, BooleanOp::Subtract).unwrap().unwrap();
    let before = volume(&thread, 1e-8).unwrap();
    let c = pitch / TAU;
    let f = |r: f64| low - t * (r - root);
    let normal_length = |r: f64| (1.0 + t * t + c * c / (r * r)).sqrt();
    // a point of the lower flank halfway across it, at mid height: turned by ψ about +z from
    // the profile's plane (the +y half of x = 0)
    let r = 0.5 * (root + crest);
    let psi = (0.5 * height - f(r)) / c;
    let pick = [-r * psi.sin(), r * psi.cos(), 0.5 * height];
    // composite Simpson's rule over [a, b] in 2n steps
    let simpson = |g: &dyn Fn(f64) -> f64, a: f64, b: f64| {
        let n = 2000;
        let h = (b - a) / f64::from(2 * n);
        let inner: f64 = (1..2 * n).map(|i| g(a + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 }).sum();
        (g(a) + g(b) + inner) * h / 3.0
    };
    let area = height / c * simpson(&|r| normal_length(r) * r, root, crest);
    for d in [0.3, -0.2] {
        // the moved point's radius, height above `c ψ` and `r′²/2`'s rate along `s`
        let moved = |s: f64| {
            let l = normal_length(s);
            let l_s = -c * c / (s * s * s) / l;
            let (a, b) = (s + d * t / l, d * c / (s * l));
            let a_s = 1.0 - d * t * l_s / (l * l);
            let b_s = -d * c * (l + s * l_s) / (s * l * s * l);
            ((a * a + b * b).sqrt(), f(s) + d / l + c * b.atan2(a), a * a_s + b * b_s)
        };
        // the radius whose point moves to `r`, by bisection (`r′` rises with `s`)
        let from = |r: f64| {
            let (mut lo, mut hi) = (r - 2.0 * d.abs(), r + 2.0 * d.abs());
            for _ in 0..200 {
                let m = 0.5 * (lo + hi);
                if moved(m).0 < r { lo = m } else { hi = m }
            }
            0.5 * (lo + hi)
        };
        let gained = simpson(&|s| moved(s).1 * moved(s).2, from(root), from(crest));
        let lost = (low + t * root) * (crest * crest - root * root) / 2.0 - t * (crest.powi(3) - root.powi(3)) / 3.0;
        let want = height / c * (gained - lost);
        let body = match offset_faces(&thread, &[pick], TOL, d) {
            Ok(body) => body,
            Err(e) => {
                // only the stages after the reach may refuse (a pick that misses, a cancel
                // or the reach itself would not): the offset's loops or its vanishing edges
                let why = e.to_string();
                let later = ["cannot be bridged", "closes to nothing", "could not be split"];
                assert!(later.iter().any(|stage| why.contains(stage)), "d {d}: {why}");
                eprintln!("d {d}: refused after the reach: {why}");
                continue;
            }
        };
        let report = super::check(&body, CheckLevel::All).unwrap();
        assert!(report.is_valid(), "d {d}: {report}");
        let after = volume(&body, 1e-8).unwrap();
        let bound = 2.0 * tolerance * area + 1e-8 * (before + after);
        let got = after - before;
        assert!((got - want).abs() <= bound, "d {d}: the volume changed by {got}, the closed form {want} (within {bound})");
    }
}

/// A draft direction of no length is refused as an argument before any face is picked, even with
/// a point that would pick no face: the direction is the first thing wrong.
#[test]
fn a_draft_with_no_neutral_normal_is_refused_before_its_pick() {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let z = [0.0, 0.0, 1.0];
    for (normal, pull) in [([0.0; 3], z), (z, [0.0; 3])] {
        assert_eq!(draft(&b, &[[50.0, 50.0, 50.0]], TOL, [0.0; 3], normal, pull, 0.1).err(), Some(Error::Argument));
    }
}
