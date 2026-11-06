//! A panic fuzz of the kernel's public entry points (#2490). On wasm32 a panic aborts
//! the application, so no input may make a public operation panic; an `Err` is fine.
//!
//! Each family generates short programs from a seed (splitmix64, no clock): primitives
//! and curves with hostile sizes (NaN, infinities, zero, negative, subnormal, huge,
//! tiny), sweeps of empty, degenerate, coincident and self-intersecting profiles along
//! zero-length or hostile directions, then booleans, fillets and chamfers, offsets and
//! shells, transforms, checks, mass properties, boundary tessellation, queries and
//! the native file reader on mutated files, on bodies made by the earlier steps. Every
//! case runs under `catch_unwind` on its own thread (`support/panic_fuzz.rs`); the test
//! fails listing each distinct panic location with a reproducing seed and input. A case
//! that runs past the hang limit is printed with its seed and input; by default it does
//! not fail the test, and it does with `SOLIDCORE_PANIC_FUZZ_HANGS=fail`.
//!
//! CI runs a fixed seed set; `SOLIDCORE_PANIC_FUZZ_CASES=<n>` runs more (see the
//! harness for the other variables).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;
#[path = "support/panic_fuzz.rs"]
mod panic_fuzz;

use panic_fuzz::{
    assert_no_findings, fuzz, parse_env_u64, parse_hangs, parse_repro, Family, Rng, Settings,
};
use solidcore::blend::fillet_executive::api as fillet;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_mass_properties_default, brep_volume_default};
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as prim;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::query::api_queries as q;
use solidcore::sweep::api_brep as sweeps;
use solidcore::sweep::api_brep::OffsetCornerType;
use solidcore::sweep::api_surfaces;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::mirror::mirror;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::{brep_copy, topology_ref};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_scale, api_scale_by_pt, api_transform, TransformObject,
};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, CancelToken, EntityRef, Kernel, Point2, Point3};

type P = [f64; 3];

fn pt(p: P) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

// ---------------------------------------------------------------------------------------
// Inputs

/// A planar profile (or path) as the caller hands it over: a list of curves.
#[derive(Clone, Debug)]
enum Profile {
    /// Line segments through the points, closed back to the first when `closed`.
    Poly { points: Vec<P>, closed: bool },
    /// `create_analytic_circle`.
    Circle { center: P, radius: f64 },
    /// `create_circle` (B-spline).
    BCircle { center: P, radius: f64 },
    /// `create_arc`.
    Arc {
        center: P,
        radius: f64,
        start: f64,
        end: f64,
    },
    /// `create_ellipse`.
    Ellipse { center: P, r1: f64, r2: f64 },
    /// `create_rectangle`.
    Rect { center: P, length: f64, width: f64 },
    /// `create_regular_polygon`.
    Polygon { center: P, sides: u64, radius: f64 },
    /// `create_curve` (interpolating) or `create_curve_approx_points`.
    Spline { points: Vec<P>, approx: bool },
    /// No curves at all.
    Empty,
}

#[derive(Clone, Debug)]
enum Prim {
    Box(P, f64, f64, f64),
    Sphere(P, f64),
    Cylinder(P, f64, f64),
    Cone(P, f64, f64, f64),
    Torus(P, f64, f64),
    Pyramid(P, f64, f64),
    Plane(P, f64, f64),
    Disc(P, f64),
    PartialSphere(P, f64, f64, f64),
    PartialCylinder(P, f64, f64, f64, f64),
    PartialCone(P, f64, f64, f64, f64, f64),
    PartialTorus(P, f64, f64, f64, f64),
    CylindricalBox(P, f64, f64, f64, f64, f64),
    SphereNoPole(P, f64, Option<f64>),
    ConeEllipticEnds([f64; 7], bool),
    Swung(Profile, Profile, f64),
    OffsetProfile(Profile, f64, u64, bool, bool),
}

#[derive(Clone, Debug)]
enum Sweep {
    Linear(Profile, P, f64, bool, Option<u64>),
    Rotational(Profile, P, P, f64, bool, Option<u64>),
    Draft(Profile, f64, f64, bool, i32),
    Pipe(f64, Profile, bool),
    PlanarPath(Profile, Profile, bool, i32),
    Curve(Profile, Profile, Option<Profile>, u8),
    Taper(Profile, f64, f64, i32, Option<u8>),
    Skin(Vec<Profile>, usize, bool),
    SurfaceRevolution(Profile, P, P, f64),
    Ruled(Profile, Profile),
}

/// One step. `body`, `a`, `b` index the bodies made so far (modulo their count);
/// edge/face/vertex lists index the body's entities (modulo their count).
#[derive(Clone, Debug)]
enum Step {
    Prim(Prim),
    Sweep(Sweep),
    Boolean {
        op: u8,
        a: usize,
        b: usize,
    },
    Boolean2d {
        op: u8,
        a: usize,
        b: usize,
    },
    MergeBreps {
        op: u8,
        bodies: Vec<usize>,
    },
    PiecewiseMerge {
        a: usize,
        b: usize,
        split: bool,
    },
    BooleanWithCurves {
        op: u8,
        a: usize,
        b: usize,
    },
    Fillet {
        body: usize,
        edges: Vec<usize>,
        radius: f64,
        xsect: u32,
        continuity: u32,
        thumb: f64,
    },
    FilletPerEdge {
        body: usize,
        edges: Vec<usize>,
        radii: Vec<f64>,
        linear: Vec<bool>,
    },
    VariableFillet {
        body: usize,
        edges: Vec<usize>,
        r0: f64,
        r1: f64,
        xsect: u32,
    },
    CircularFillet {
        body: usize,
        radius: f64,
    },
    ChamferFillet {
        body: usize,
        distance: f64,
    },
    Bevel {
        body: usize,
        edges: Vec<usize>,
        radius: f64,
        vertices: Vec<usize>,
    },
    Preview {
        body: usize,
        edges: Vec<usize>,
        radius: f64,
    },
    Offset {
        body: usize,
        distance: f64,
    },
    Shell {
        body: usize,
        distance: f64,
        open: Vec<usize>,
        solid: bool,
    },
    Transform {
        body: usize,
        origin: P,
        x: P,
        y: P,
    },
    Scale {
        body: usize,
        scale: P,
        about: Option<P>,
    },
    Mirror {
        body: usize,
        origin: P,
        x: P,
        y: P,
    },
    Check {
        body: usize,
        level: u8,
    },
    Mass {
        body: usize,
        accuracy: f64,
        density: f64,
        origin: P,
    },
    Volume {
        body: usize,
        accuracy: f64,
    },
    Tessellate {
        body: usize,
        angle: f64,
    },
    Queries {
        body: usize,
        point: P,
        param: f64,
        uv: [f64; 2],
        pick: usize,
    },
    FileRoundTrip {
        body: usize,
        ascii: bool,
        edits: Vec<(f64, u8, u64)>,
    },
}

/// What a family's programs are made of.
#[derive(Clone, Copy, Debug)]
enum Mix {
    Primitives,
    Sweeps,
    Booleans,
    Blends,
    Offsets,
    Transforms,
    Analysis,
    Files,
}

// ---------------------------------------------------------------------------------------
// Generation

/// A plausible size most of the time, a hostile value with probability `p`.
fn size(r: &mut Rng, p: f64) -> f64 {
    r.hostile(p, 0.1, 10.0)
}

fn coord(r: &mut Rng, p: f64) -> f64 {
    r.hostile(p, -5.0, 5.0)
}

fn point(r: &mut Rng, p: f64) -> P {
    [coord(r, p), coord(r, p), coord(r, p)]
}

fn angle(r: &mut Rng, p: f64) -> f64 {
    if r.chance(p) {
        r.pick(&[
            f64::NAN,
            f64::INFINITY,
            -f64::INFINITY,
            0.0,
            -0.0,
            360.0,
            -360.0,
            720.0,
            1e-12,
            1e300,
            -90.0,
            180.0,
            5e-324,
        ])
    } else {
        r.range(-30.0, 400.0)
    }
}

fn direction(r: &mut Rng, p: f64) -> P {
    if r.chance(p) {
        r.pick(&[
            [0.0, 0.0, 0.0],
            [f64::NAN, 0.0, 1.0],
            [0.0, 0.0, f64::INFINITY],
            [1e-300, 0.0, 0.0],
            [5e-324, 5e-324, 5e-324],
            [1e300, 1e300, 1e300],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
        ])
    } else if r.chance(0.6) {
        [0.0, 0.0, 1.0]
    } else {
        [r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0)]
    }
}

/// Planar (z = 0) points: a regular polygon, a self-intersecting bow tie, coincident or
/// collinear points, or random ones; any coordinate may be hostile.
fn planar_points(r: &mut Rng, p: f64) -> Vec<P> {
    let n = 1 + r.below(7);
    let s = size(r, p);
    let mut pts: Vec<P> = match r.below(5) {
        0 => (0..n)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / n as f64;
                [s * t.cos(), s * t.sin(), 0.0]
            })
            .collect(),
        1 => vec![[0.0, 0.0, 0.0], [s, s, 0.0], [s, 0.0, 0.0], [0.0, s, 0.0]],
        2 => vec![[1.0, 1.0, 0.0]; n],
        3 => (0..n).map(|i| [i as f64 * s, 0.0, 0.0]).collect(),
        _ => (0..n).map(|_| [coord(r, p), coord(r, p), 0.0]).collect(),
    };
    if r.chance(p) {
        let i = r.below(pts.len());
        let j = r.below(3);
        pts[i][j] = r.pick(panic_fuzz::HOSTILE);
    }
    if r.chance(p) && !pts.is_empty() {
        let dup = pts[r.below(pts.len())];
        pts.insert(r.below(pts.len() + 1), dup);
    }
    pts
}

fn profile(r: &mut Rng, p: f64) -> Profile {
    let c = if r.chance(0.7) { [0.0; 3] } else { point(r, p) };
    match r.below(10) {
        0..=2 => Profile::Poly {
            points: planar_points(r, p),
            closed: !r.chance(p),
        },
        3 => Profile::Circle {
            center: c,
            radius: size(r, p),
        },
        4 => Profile::BCircle {
            center: c,
            radius: size(r, p),
        },
        5 => Profile::Arc {
            center: c,
            radius: size(r, p),
            start: angle(r, p),
            end: angle(r, p),
        },
        6 => Profile::Ellipse {
            center: c,
            r1: size(r, p),
            r2: size(r, p),
        },
        7 => Profile::Rect {
            center: c,
            length: size(r, p),
            width: size(r, p),
        },
        8 => Profile::Polygon {
            center: c,
            sides: r.pick(&[0, 1, 2, 3, 4, 6, 64, u64::MAX]),
            radius: size(r, p),
        },
        _ => {
            if r.chance(0.2) {
                Profile::Empty
            } else {
                Profile::Spline {
                    points: planar_points(r, p),
                    approx: r.chance(0.5),
                }
            }
        }
    }
}

fn prim(r: &mut Rng, p: f64) -> Prim {
    let o = if r.chance(0.6) { [0.0; 3] } else { point(r, p) };
    match r.below(17) {
        0 => Prim::Box(o, size(r, p), size(r, p), size(r, p)),
        1 => Prim::Sphere(o, size(r, p)),
        2 => Prim::Cylinder(o, size(r, p), size(r, p)),
        3 => Prim::Cone(o, size(r, p), size(r, p), size(r, p)),
        4 => Prim::Torus(o, size(r, p), size(r, p)),
        5 => Prim::Pyramid(o, size(r, p), size(r, p)),
        6 => Prim::Plane(o, size(r, p), size(r, p)),
        7 => Prim::Disc(o, size(r, p)),
        8 => Prim::PartialSphere(o, size(r, p), angle(r, p), angle(r, p)),
        9 => Prim::PartialCylinder(o, size(r, p), size(r, p), angle(r, p), angle(r, p)),
        10 => Prim::PartialCone(
            o,
            size(r, p),
            size(r, p),
            size(r, p),
            angle(r, p),
            angle(r, p),
        ),
        11 => Prim::PartialTorus(o, size(r, p), size(r, p), angle(r, p), angle(r, p)),
        12 => Prim::CylindricalBox(
            o,
            size(r, p),
            size(r, p),
            size(r, p),
            angle(r, p),
            angle(r, p),
        ),
        13 => Prim::SphereNoPole(
            o,
            size(r, p),
            r.chance(0.5).then(|| r.hostile(p, 1e-6, 1e-3)),
        ),
        14 => Prim::ConeEllipticEnds(
            [
                size(r, p),
                size(r, p),
                size(r, p),
                angle(r, p) / 10.0,
                angle(r, p) / 10.0,
                angle(r, p) / 10.0,
                angle(r, p) / 10.0,
            ],
            r.chance(0.7),
        ),
        15 => Prim::Swung(profile(r, p), profile(r, p), size(r, p)),
        _ => Prim::OffsetProfile(
            profile(r, p),
            r.hostile(p, -1.0, 1.0),
            r.pick(&[0, 1, 2, 3, 4, 99]),
            r.chance(0.5),
            r.chance(0.5),
        ),
    }
}

fn sweep(r: &mut Rng, p: f64) -> Sweep {
    let reps = |r: &mut Rng| r.chance(0.2).then(|| r.pick(&[0, 1, 2, 3, 5]));
    match r.below(10) {
        0 | 1 => Sweep::Linear(
            profile(r, p),
            direction(r, p),
            size(r, p),
            r.chance(0.8),
            reps(r),
        ),
        2 => Sweep::Rotational(
            profile(r, p),
            if r.chance(0.5) {
                [20.0, 0.0, 0.0]
            } else {
                point(r, p)
            },
            if r.chance(0.6) {
                [0.0, 1.0, 0.0]
            } else {
                direction(r, p)
            },
            angle(r, p),
            r.chance(0.8),
            reps(r),
        ),
        3 => Sweep::Draft(
            profile(r, p),
            size(r, p),
            angle(r, p) / 10.0,
            r.chance(0.5),
            r.pick(&[0, 1, 2, 3, -1, 7]),
        ),
        4 => Sweep::Pipe(size(r, p), profile(r, p), r.chance(0.8)),
        5 => Sweep::PlanarPath(
            profile(r, p),
            profile(r, p),
            r.chance(0.5),
            r.pick(&[0, 1, 2, 3, -1]),
        ),
        6 => Sweep::Curve(
            profile(r, p),
            profile(r, p),
            r.chance(0.2).then(|| profile(r, p)),
            r.next_u64() as u8,
        ),
        7 => Sweep::Taper(
            profile(r, p),
            size(r, p),
            angle(r, p) / 10.0,
            r.pick(&[0, 1, 2, 3, -1]),
            r.chance(0.5).then(|| r.below(5) as u8),
        ),
        8 => {
            let n = r.below(5);
            Sweep::Skin(
                (0..n)
                    .map(|i| {
                        let mut pr = profile(r, p);
                        if let Profile::Circle { center, .. } | Profile::BCircle { center, .. } =
                            &mut pr
                        {
                            center[2] = i as f64 * 2.0;
                        }
                        pr
                    })
                    .collect(),
                r.pick(&[0, 1, 2, 3, 5, 99]),
                r.chance(0.5),
            )
        }
        _ => {
            if r.chance(0.5) {
                Sweep::SurfaceRevolution(profile(r, p), point(r, p), direction(r, p), angle(r, p))
            } else {
                Sweep::Ruled(profile(r, p), profile(r, p))
            }
        }
    }
}

fn picks(r: &mut Rng, max: usize) -> Vec<usize> {
    (0..r.below(max + 1)).map(|_| r.below(64)).collect()
}

/// A body-making step: a primitive (most of the time a valid one), sometimes a sweep.
fn body_step(r: &mut Rng, p: f64) -> Step {
    if r.chance(0.8) {
        Step::Prim(prim(r, p))
    } else {
        Step::Sweep(sweep(r, p))
    }
}

fn op_step(r: &mut Rng, mix: Mix, p: f64) -> Step {
    let body = r.below(8);
    match mix {
        Mix::Primitives => Step::Prim(prim(r, p)),
        Mix::Sweeps => Step::Sweep(sweep(r, p)),
        Mix::Booleans => match r.below(6) {
            0..=2 => Step::Boolean {
                op: r.below(9) as u8,
                a: r.below(8),
                b: r.below(8),
            },
            3 => Step::Boolean2d {
                op: r.below(6) as u8,
                a: r.below(8),
                b: r.below(8),
            },
            4 => Step::MergeBreps {
                op: r.below(9) as u8,
                bodies: picks(r, 4),
            },
            _ => {
                if r.chance(0.5) {
                    Step::PiecewiseMerge {
                        a: r.below(8),
                        b: r.below(8),
                        split: r.chance(0.5),
                    }
                } else {
                    Step::BooleanWithCurves {
                        op: r.below(9) as u8,
                        a: r.below(8),
                        b: r.below(8),
                    }
                }
            }
        },
        Mix::Blends => {
            let radius = r.hostile(p, 0.01, 2.0);
            match r.below(8) {
                0..=2 => Step::Fillet {
                    body,
                    edges: picks(r, 4),
                    radius,
                    xsect: r.pick(&[1, 2, 2, 3, 0, 9]),
                    continuity: r.pick(&[1, 1, 0, 2, 7]),
                    thumb: r.hostile(p, 0.5, 2.0),
                },
                3 => {
                    let edges = picks(r, 4);
                    let n = if r.chance(0.8) {
                        edges.len()
                    } else {
                        r.below(5)
                    };
                    Step::FilletPerEdge {
                        body,
                        radii: (0..n).map(|_| r.hostile(p, 0.01, 2.0)).collect(),
                        linear: (0..n).map(|_| r.chance(0.3)).collect(),
                        edges,
                    }
                }
                4 => Step::VariableFillet {
                    body,
                    edges: picks(r, 3),
                    r0: radius,
                    r1: r.hostile(p, 0.01, 2.0),
                    xsect: r.pick(&[1, 2, 3]),
                },
                5 => {
                    if r.chance(0.5) {
                        Step::CircularFillet { body, radius }
                    } else {
                        Step::ChamferFillet {
                            body,
                            distance: radius,
                        }
                    }
                }
                6 => Step::Bevel {
                    body,
                    edges: picks(r, 3),
                    radius,
                    vertices: picks(r, 2),
                },
                _ => Step::Preview {
                    body,
                    edges: picks(r, 3),
                    radius,
                },
            }
        }
        Mix::Offsets => {
            let distance = r.hostile(p, -1.0, 1.0);
            if r.chance(0.5) {
                Step::Offset { body, distance }
            } else {
                Step::Shell {
                    body,
                    distance,
                    open: picks(r, 2),
                    solid: r.chance(0.3),
                }
            }
        }
        Mix::Transforms => match r.below(3) {
            0 => Step::Transform {
                body,
                origin: point(r, p),
                x: direction(r, p),
                y: direction(r, p),
            },
            1 => Step::Scale {
                body,
                scale: [size(r, p), size(r, p), size(r, p)],
                about: r.chance(0.5).then(|| point(r, p)),
            },
            _ => Step::Mirror {
                body,
                origin: point(r, p),
                x: direction(r, p),
                y: direction(r, p),
            },
        },
        Mix::Analysis => match r.below(6) {
            0 => Step::Check {
                body,
                level: r.below(3) as u8,
            },
            1 => Step::Mass {
                body,
                accuracy: r.hostile(p, 1e-4, 1e-2),
                density: r.hostile(p, 0.5, 2.0),
                origin: point(r, p),
            },
            2 => Step::Volume {
                body,
                accuracy: r.hostile(p, 1e-4, 1e-2),
            },
            3 => Step::Tessellate {
                body,
                angle: angle(r, p) / 10.0,
            },
            _ => Step::Queries {
                body,
                point: point(r, p),
                param: r.hostile(p, -1.0, 2.0),
                uv: [r.hostile(p, -1.0, 2.0), r.hostile(p, -1.0, 2.0)],
                pick: r.below(64),
            },
        },
        Mix::Files => Step::FileRoundTrip {
            body,
            ascii: r.chance(0.5),
            edits: (0..r.below(4))
                .map(|_| (r.unit(), r.below(6) as u8, r.next_u64()))
                .collect(),
        },
    }
}

/// A program: one to three body-making steps, then (except for the body-making
/// families) one to three operations, some of them on earlier results.
fn program(r: &mut Rng, mix: Mix) -> Vec<Step> {
    // most programs keep most values sane so they get deep into an operation
    let p = r.pick(&[0.0, 0.05, 0.15, 0.4]);
    match mix {
        Mix::Primitives | Mix::Sweeps => (0..=r.below(2)).map(|_| op_step(r, mix, p)).collect(),
        _ => {
            let mut steps: Vec<Step> = (0..=r.below(3)).map(|_| body_step(r, p / 2.0)).collect();
            for _ in 0..=r.below(3) {
                steps.push(op_step(r, mix, p));
                if r.chance(0.2) {
                    steps.push(op_step(r, Mix::Analysis, p));
                }
            }
            steps
        }
    }
}

// ---------------------------------------------------------------------------------------
// Execution

fn curves_of(k: &Kernel, profile: &Profile) -> Option<Vec<Curve>> {
    let line = |a: P, b: P| {
        curves::create_line_segment(pt(a), pt(b))
            .ok()
            .map(Curve::from)
    };
    Some(match profile {
        Profile::Poly { points, closed } => {
            let n = points.len();
            let segments = if *closed { n } else { n.saturating_sub(1) };
            let mut out = Vec::new();
            for i in 0..segments {
                out.push(line(points[i], points[(i + 1) % n])?);
            }
            out
        }
        Profile::Circle { center, radius } => {
            vec![Curve::from(
                curves::create_analytic_circle(k, pt(*center), *radius).ok()?,
            )]
        }
        Profile::BCircle { center, radius } => {
            vec![Curve::from(
                curves::create_circle(pt(*center), *radius).ok()?,
            )]
        }
        Profile::Arc {
            center,
            radius,
            start,
            end,
        } => vec![Curve::from(
            curves::create_arc(pt(*center), *radius, *start, *end).ok()?,
        )],
        Profile::Ellipse { center, r1, r2 } => {
            vec![Curve::from(
                curves::create_ellipse(k, pt(*center), *r1, *r2).ok()?,
            )]
        }
        Profile::Rect {
            center,
            length,
            width,
        } => curves::create_rectangle(pt(*center), *length, *width)
            .ok()?
            .into_iter()
            .map(Curve::from)
            .collect(),
        Profile::Polygon {
            center,
            sides,
            radius,
        } => curves::create_regular_polygon(pt(*center), *sides, *radius)
            .ok()?
            .into_iter()
            .map(Curve::from)
            .collect(),
        Profile::Spline { points, approx } => {
            let pts: Vec<Point3> = points.iter().copied().map(pt).collect();
            let c = if *approx {
                curves::create_curve_approx_points(&pts)
            } else {
                curves::create_curve(&pts)
            };
            vec![Curve::from(c.ok()?)]
        }
        Profile::Empty => Vec::new(),
    })
}

fn bspline_of(
    k: &Kernel,
    profile: &Profile,
) -> Option<solidcore::nurbs::b_spline_curve::BSplineCurve> {
    let pts = match profile {
        Profile::Spline { points, .. } | Profile::Poly { points, .. } => points.clone(),
        Profile::Arc {
            center,
            radius,
            start,
            end,
        } => return curves::create_arc(pt(*center), *radius, *start, *end).ok(),
        _ => {
            let _ = k;
            return curves::create_circle(pt([0.0; 3]), 1.0).ok();
        }
    };
    curves::create_curve(&pts.into_iter().map(pt).collect::<Vec<_>>()).ok()
}

fn boolean_op(op: u8) -> BooleanOperation {
    use BooleanOperation as B;
    [
        B::Unknown,
        B::Union,
        B::Intersection,
        B::Difference,
        B::ExclusiveOr,
        B::Merge,
        B::PartialMerge,
        B::Imprint,
        B::Union,
    ][usize::from(op) % 9]
}

fn boolean_2d(op: u8) -> boolean::Boolean2d {
    use boolean::Boolean2d as B;
    [
        B::Union,
        B::Intersection,
        B::Difference,
        B::ExclusiveOr,
        B::Union,
        B::Difference,
    ][usize::from(op) % 6]
}

fn run_prim(k: &mut Kernel, p: &Prim) -> solidcore::Result<Brep> {
    match p {
        Prim::Box(o, l, w, h) => prim::create_box(k, pt(*o), *l, *w, *h),
        Prim::Sphere(o, r) => prim::create_sphere(k, pt(*o), *r),
        Prim::Cylinder(o, r, h) => prim::create_cylinder(k, pt(*o), *r, *h),
        Prim::Cone(o, a, b, h) => prim::create_cone(k, pt(*o), *a, *b, *h),
        Prim::Torus(o, a, b) => prim::create_torus(k, pt(*o), *a, *b),
        Prim::Pyramid(o, l, h) => prim::create_pyramid(k, pt(*o), *l, *h),
        Prim::Plane(o, x, y) => prim::create_plane(k, pt(*o), *x, *y),
        Prim::Disc(o, r) => prim::create_planar_circle(k, pt(*o), *r),
        Prim::PartialSphere(o, r, a, b) => prim::create_partial_sphere(k, pt(*o), *r, *a, *b),
        Prim::PartialCylinder(o, r, h, a, b) => {
            prim::create_partial_cylinder(k, pt(*o), *r, *h, *a, *b)
        }
        Prim::PartialCone(o, r0, r1, h, a, b) => {
            prim::create_partial_cone(k, pt(*o), *r0, *r1, *h, *a, *b)
        }
        Prim::PartialTorus(o, r0, r1, a, b) => {
            prim::create_partial_torus(k, pt(*o), *r0, *r1, *a, *b)
        }
        Prim::CylindricalBox(o, l, r0, r1, a, b) => {
            prim::create_cylindrical_box(k, pt(*o), *l, *r0, *r1, *a, *b)
        }
        Prim::SphereNoPole(o, r, None) => prim::create_sphere_no_pole(k, pt(*o), *r),
        Prim::SphereNoPole(o, r, Some(t)) => {
            prim::create_sphere_no_pole_with_tolerance(k, pt(*o), *r, *t)
        }
        Prim::ConeEllipticEnds(v, caps) => prim::create_cone_elliptic_ends(
            k,
            &prim::ConeEllipticEnds {
                front_radius: v[0],
                rear_radius: v[1],
                height: v[2],
                front_y_tilt_deg: v[3],
                rear_y_tilt_deg: v[4],
                front_x_tilt_deg: v[5],
                rear_x_tilt_deg: v[6],
                end_caps: *caps,
            },
        ),
        Prim::Swung(a, b, s) => {
            let (Some(a), Some(b)) = (curves_of(k, a), curves_of(k, b)) else {
                return Err(solidcore::SolidError::sm_err("fuzz"));
            };
            prim::create_swung_primitive(k, &a, &b, *s)
        }
        Prim::OffsetProfile(pr, d, t, round, shell) => {
            let cs = curves_of(k, pr).unwrap_or_default();
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            prim::create_offset_profile(k, &refs, *d, *t, *round, *shell)
        }
    }
}

fn run_sweep(k: &mut Kernel, s: &Sweep) -> solidcore::Result<Option<Brep>> {
    let none = || solidcore::SolidError::sm_err("fuzz");
    let cs = |k: &Kernel, pr: &Profile| curves_of(k, pr).ok_or_else(none);
    Ok(Some(match s {
        Sweep::Linear(pr, dir, dist, cap, reps) => {
            let c = cs(k, pr)?;
            let refs: Vec<Option<&Curve>> = c.iter().map(Some).collect();
            match reps {
                None => sweeps::create_linear_sweep(k, &refs, pt(*dir), *dist, *cap)?,
                Some(n) => sweeps::create_linear_sweep_with_repetitions(
                    k,
                    &refs,
                    pt(*dir),
                    *dist,
                    *n,
                    *cap,
                )?,
            }
        }
        Sweep::Rotational(pr, base, axis, ang, cap, reps) => {
            let c = cs(k, pr)?;
            let refs: Vec<Option<&Curve>> = c.iter().map(Some).collect();
            match reps {
                None => {
                    sweeps::create_rotational_sweep(k, &refs, pt(*base), pt(*axis), *ang, *cap)?
                }
                Some(n) => sweeps::create_rotational_sweep_with_repetitions(
                    k,
                    &refs,
                    pt(*base),
                    pt(*axis),
                    *ang,
                    *n,
                    *cap,
                )?,
            }
        }
        Sweep::Draft(pr, h, a, corner, caps) => {
            let c = cs(k, pr)?;
            let refs: Vec<Option<&Curve>> = c.iter().map(Some).collect();
            sweeps::create_draft_sweep(k, &refs, *h, *a, *corner, *caps)?
        }
        Sweep::Pipe(radius, path, cap) => {
            let c = cs(k, path)?;
            sweeps::create_pipe_sweep(k, *radius, c.first(), *cap)?
        }
        Sweep::PlanarPath(pr, path, mv, caps) => {
            let c = cs(k, pr)?;
            let pc = cs(k, path)?;
            let a: Vec<&Curve> = c.iter().collect();
            let b: Vec<&Curve> = pc.iter().collect();
            sweeps::create_sweep_along_planar_path(k, &a, &b, *mv, *caps)?
        }
        Sweep::Curve(pr, path, scale, bits) => {
            let c = cs(k, pr)?;
            let pc = cs(k, path)?;
            let sc = match scale {
                Some(s) => Some(cs(k, s)?),
                None => None,
            };
            let refs: Vec<Option<&Curve>> = c.iter().map(Some).collect();
            let options = SweepOptions {
                cap_ends: bits & 1 != 0,
                translational_sweep: bits & 2 != 0,
                move_profile_center_to_path: bits & 4 != 0,
                orient_profile_perpendicular_to_path: bits & 8 != 0,
                move_to_profile: bits & 16 != 0,
                profile_at_path_start: bits & 32 != 0,
            };
            let sc0 = sc.as_ref().and_then(|v| v.first());
            let sc1 = sc.as_ref().and_then(|v| v.get(1));
            sweeps::create_curve_sweep(k, &refs, pc.first(), sc0, sc1, Some(&options))?.brep
        }
        Sweep::Taper(pr, h, a, caps, corner) => {
            let c = cs(k, pr)?;
            let refs: Vec<Option<&Curve>> = c.iter().map(Some).collect();
            match corner {
                None => sweeps::create_taper_extrude(k, &refs, *h, *a, *caps, *caps > 1)?,
                Some(t) => {
                    let corner = [
                        OffsetCornerType::LinearExtension,
                        OffsetCornerType::FilletCorner,
                        OffsetCornerType::LinearChamfer,
                    ][usize::from(*t) % 3];
                    match sweeps::create_taper_extrude_with_corner(k, &refs, *h, *a, *caps, corner)?
                    {
                        sweeps::TaperExtrude::Solid(b) => b,
                        sweeps::TaperExtrude::Refused(_) => return Ok(None),
                    }
                }
            }
        }
        Sweep::Skin(sections, degree, cap) => {
            let mut all = Vec::new();
            let mut counts = Vec::new();
            for s in sections {
                let c = cs(k, s)?;
                counts.push(c.len());
                all.extend(c);
            }
            let refs: Vec<Option<&Curve>> = all.iter().map(Some).collect();
            api_surfaces::create_skin_primitive(k, &counts, &refs, *degree, *cap)?.brep
        }
        Sweep::SurfaceRevolution(pr, o, axis, ang) => {
            let c = bspline_of(k, pr);
            api_surfaces::create_surface_revolution(k, c.as_ref(), pt(*o), pt(*axis), *ang)?;
            return Ok(None);
        }
        Sweep::Ruled(a, b) => {
            if let (Some(a), Some(b)) = (bspline_of(k, a), bspline_of(k, b)) {
                api_surfaces::create_ruled_surface(&a, &b)?;
                let _ = api_surfaces::create_skin(k, &[a.clone(), b.clone()]);
                let _ = api_surfaces::create_sweep(&a, &b);
            }
            return Ok(None);
        }
    }))
}

fn nth<T: Copy>(items: &[T], i: usize) -> Option<T> {
    if items.is_empty() {
        None
    } else {
        Some(items[i % items.len()])
    }
}

fn picked_edges(b: &Brep, picks: &[usize]) -> Vec<EdgeId> {
    let edges = q::get_edges(b).unwrap_or_default();
    picks.iter().filter_map(|&i| nth(&edges, i)).collect()
}

fn picked_faces(b: &Brep, picks: &[usize]) -> Vec<FaceId> {
    let faces = q::get_faces(b).unwrap_or_default();
    picks.iter().filter_map(|&i| nth(&faces, i)).collect()
}

fn caller_named(entity: EntityRef) -> ObjectRef {
    let r = topology_ref(0, entity);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(r.type_code.unsigned_abs() * 1_000_000 + (r.key.0 & 0xffff_ffff)),
    }
}

/// `edits`: (where in the file as a fraction, what, a random word).
fn mutate(bytes: &mut Vec<u8>, edits: &[(f64, u8, u64)], ascii: bool) {
    for &(at, what, word) in edits {
        if bytes.is_empty() {
            return;
        }
        let i = ((at * bytes.len() as f64) as usize).min(bytes.len() - 1);
        match what {
            0 => bytes.truncate(i),
            1 => bytes[i] ^= 1 << (word % 8),
            2 => {
                // a huge count / length / index
                let huge = if ascii {
                    b"4294967295 ".to_vec()
                } else {
                    word.to_le_bytes().to_vec()
                };
                for (j, b) in huge.into_iter().enumerate() {
                    if let Some(slot) = bytes.get_mut(i + j) {
                        *slot = b;
                    }
                }
            }
            3 => {
                // NaN / inf
                let bad: Vec<u8> = if ascii {
                    if word % 2 == 0 {
                        b"nan ".to_vec()
                    } else {
                        b"-inf".to_vec()
                    }
                } else {
                    f64::NAN.to_le_bytes().to_vec()
                };
                for (j, b) in bad.into_iter().enumerate() {
                    if let Some(slot) = bytes.get_mut(i + j) {
                        *slot = b;
                    }
                }
            }
            4 => {
                let len = (word as usize % 64).min(bytes.len() - i);
                bytes.drain(i..i + len);
            }
            _ => {
                // a zero or negative small number (counts of 0, -1)
                let v: &[u8] = if ascii { b"-1 " } else { &[0, 0, 0, 0] };
                for (j, b) in v.iter().enumerate() {
                    if let Some(slot) = bytes.get_mut(i + j) {
                        *slot = *b;
                    }
                }
            }
        }
    }
}

fn file_round_trip(k: &mut Kernel, b: &Brep, ascii: bool, edits: &[(f64, u8, u64)]) {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let own = |e: EntityRef| b.owner_identity(e);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &own,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let Ok(mut bytes) = write_brep_to_file(k, Some(b), &env, ascii) else {
        return;
    };
    mutate(&mut bytes, edits, ascii);
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &caller_named,
        records: &mut records,
        next_key: 1 << 50,
    };
    if let Ok(mut read) = read_brep_from_file(k, Some(bytes), ascii, false, &mut env) {
        read.refresh_cached_topology();
        let _ = q::brep_bounding_box(k, &read, false);
        let _ = brep_volume_default(k, &mut read, 1e-3);
    }
}

fn queries(k: &mut Kernel, b: &mut Brep, point: P, param: f64, uv: [f64; 2], pick: usize) {
    let p = pt(point);
    let _ = q::brep_bounding_box(k, b, pick % 2 == 0);
    let _ = q::brep_is_manifold_solid(b);
    let _ = q::brep_material_census(b);
    let _ = q::find_edge(k, b, p);
    if let Some(e) = nth(&q::get_edges(b).unwrap_or_default(), pick) {
        let _ = q::edge_bounding_box(k, b, e, true);
        let _ = q::edge_closest_point(k, b, e, p);
        let _ = q::edge_parameter_range(b, e);
        let _ = q::edge_classify_parameter(k, b, e, param);
        let _ = q::edge_tangent(k, b, e, param);
    }
    if let Some(f) = nth(&q::get_faces(b).unwrap_or_default(), pick) {
        let _ = q::face_bounding_box(k, b, f, true);
        let _ = q::face_get_boundary_loops(b, f);
        let _ = q::face_internal_point(k, b, f);
        let _ = q::face_classify_uv(k, b, f, Point2::new(uv[0], uv[1]));
    }
}

/// Runs a program; errors end a step, never the program.
fn execute(steps: &[Step], cancel: &CancelToken) {
    let mut k = Kernel::default();
    k.set_cancel_token(cancel.clone());
    let k = &mut k;
    let mut bodies: Vec<Brep> = Vec::new();
    for step in steps {
        if cancel.is_cancelled() {
            return;
        }
        let n = bodies.len();
        let made: Option<Brep> = match step {
            Step::Prim(p) => run_prim(k, p).ok(),
            Step::Sweep(s) => run_sweep(k, s).ok().flatten(),
            Step::Boolean { op, a, b } if n > 0 => {
                let (Ok(x), Ok(y)) = (brep_copy(k, &bodies[a % n]), brep_copy(k, &bodies[b % n]))
                else {
                    continue;
                };
                boolean::boolean(k, x, y, boolean_op(*op))
                    .ok()
                    .map(|o| o.result)
            }
            Step::Boolean2d { op, a, b } if n > 0 => {
                let (Ok(x), Ok(y)) = (brep_copy(k, &bodies[a % n]), brep_copy(k, &bodies[b % n]))
                else {
                    continue;
                };
                match boolean::boolean_2d(k, x, y, boolean_2d(*op)) {
                    Ok(boolean::Boolean2dOutcome::Result(b)) => Some(b),
                    _ => None,
                }
            }
            Step::MergeBreps { op, bodies: which } if n > 0 => {
                let Ok(mut list) = which
                    .iter()
                    .map(|i| brep_copy(k, &bodies[i % n]))
                    .collect::<solidcore::Result<Vec<Brep>>>()
                else {
                    continue;
                };
                boolean::merge_breps(k, &mut list, boolean_op(*op)).ok()
            }
            Step::PiecewiseMerge { a, b, split } if n > 0 => {
                let (Ok(x), Ok(y)) = (brep_copy(k, &bodies[a % n]), brep_copy(k, &bodies[b % n]))
                else {
                    continue;
                };
                let _ = boolean::piecewise_merge(k, x, y, *split);
                None
            }
            Step::BooleanWithCurves { op, a, b } if n > 0 => {
                let (Ok(x), Ok(y)) = (brep_copy(k, &bodies[a % n]), brep_copy(k, &bodies[b % n]))
                else {
                    continue;
                };
                boolean::boolean_with_curves(k, x, y, boolean_op(*op))
                    .ok()
                    .map(|o| o.outcome.result)
            }
            Step::Fillet {
                body,
                edges,
                radius,
                xsect,
                continuity,
                thumb,
            } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let e = picked_edges(&b, edges);
                fillet::fillet_edges(k, &mut b, &e, *radius, *xsect, *continuity, *thumb)
                    .ok()
                    .map(|()| b)
            }
            Step::FilletPerEdge {
                body,
                edges,
                radii,
                linear,
            } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let e = picked_edges(&b, edges);
                let types: Vec<FilletSurfaceGeneratorType> = linear
                    .iter()
                    .map(|&l| {
                        if l {
                            FilletSurfaceGeneratorType::Linear
                        } else {
                            FilletSurfaceGeneratorType::Circular
                        }
                    })
                    .collect();
                fillet::fillet_edges_per_edge(k, &mut b, &e, radii, &types, 1, 1.0)
                    .ok()
                    .map(|()| b)
            }
            Step::VariableFillet {
                body,
                edges,
                r0,
                r1,
                xsect,
            } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let e = picked_edges(&b, edges);
                fillet::variable_radius_fillet(k, &mut b, &e, *r0, *r1, *xsect, 1)
                    .ok()
                    .map(|()| b)
            }
            Step::CircularFillet { body, radius } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                fillet::circular_fillet(k, &mut b, *radius).ok().map(|()| b)
            }
            Step::ChamferFillet { body, distance } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                fillet::chamfer_fillet(k, &mut b, *distance)
                    .ok()
                    .map(|()| b)
            }
            Step::Bevel {
                body,
                edges,
                radius,
                vertices,
            } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let e = picked_edges(&b, edges);
                let all: Vec<_> = b.vertices().ids().collect();
                let v: Vec<_> = vertices.iter().filter_map(|&i| nth(&all, i)).collect();
                fillet::set_bevel_corners(k, &mut b, &e, *radius, &v)
                    .ok()
                    .map(|()| b)
            }
            Step::Preview {
                body,
                edges,
                radius,
            } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let e = picked_edges(&b, edges);
                let _ = fillet::fillet_preview(k, &mut b, &e, *radius);
                None
            }
            Step::Offset { body, distance } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                offset_fixture::offset_full(k, &mut b, *distance).ok()
            }
            Step::Shell {
                body,
                distance,
                open,
                solid,
            } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let faces = picked_faces(&b, open);
                offset_fixture::api_shell(
                    k,
                    &mut b,
                    ShellOptions {
                        offset: OffsetOptions {
                            distance: *distance,
                            extend_convex_edges: true,
                            merge_results: true,
                        },
                        create_offset_solid: *solid,
                        merge_sheet_side_walls: false,
                    },
                    &faces,
                )
                .ok()
            }
            Step::Transform { body, origin, x, y } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let placement = Axis2Placement::new(pt(*origin), pt(*x), pt(*y));
                api_transform(k, Some(TransformObject::Brep(&mut b)), &placement)
                    .ok()
                    .map(|()| b)
            }
            Step::Scale { body, scale, about } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let r = match about {
                    None => api_scale(k, Some(TransformObject::Brep(&mut b)), pt(*scale)),
                    Some(o) => {
                        api_scale_by_pt(k, Some(TransformObject::Brep(&mut b)), pt(*o), pt(*scale))
                    }
                };
                r.ok().map(|()| b)
            }
            Step::Mirror { body, origin, x, y } if n > 0 => {
                let Ok(mut b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                let placement = Axis2Placement::new(pt(*origin), pt(*x), pt(*y));
                mirror(k, &mut b, &placement).ok().map(|()| b)
            }
            Step::Check { body, level } if n > 0 => {
                let Ok(mut b) = bodies[body % n].clone_standing_for_self() else {
                    continue;
                };
                let level = [CheckLevel::Level0, CheckLevel::Level1, CheckLevel::Level2]
                    [usize::from(*level) % 3];
                let _ = check_brep(k, &mut b, level);
                None
            }
            Step::Mass {
                body,
                accuracy,
                density,
                origin,
            } if n > 0 => {
                let Ok(mut b) = bodies[body % n].clone_standing_for_self() else {
                    continue;
                };
                let _ = brep_mass_properties_default(k, &mut b, *accuracy, *density, pt(*origin));
                None
            }
            Step::Volume { body, accuracy } if n > 0 => {
                let Ok(mut b) = bodies[body % n].clone_standing_for_self() else {
                    continue;
                };
                let _ = brep_volume_default(k, &mut b, *accuracy);
                None
            }
            Step::Tessellate { body, angle } if n > 0 => {
                let _ = boolean::tessellate_boundaries(k, &bodies[body % n], *angle);
                None
            }
            Step::Queries {
                body,
                point,
                param,
                uv,
                pick,
            } if n > 0 => {
                let Ok(mut b) = bodies[body % n].clone_standing_for_self() else {
                    continue;
                };
                queries(k, &mut b, *point, *param, *uv, *pick);
                None
            }
            Step::FileRoundTrip { body, ascii, edits } if n > 0 => {
                let Ok(b) = brep_copy(k, &bodies[body % n]) else {
                    continue;
                };
                file_round_trip(k, &b, *ascii, edits);
                None
            }
            _ => None,
        };
        if let Some(b) = made {
            bodies.push(b);
        }
    }
}

struct Programs {
    name: &'static str,
    mix: Mix,
}

impl Family for Programs {
    type Input = Vec<Step>;

    fn name(&self) -> &'static str {
        self.name
    }

    fn generate(&self, rng: &mut Rng) -> Vec<Step> {
        program(rng, self.mix)
    }

    fn run(input: Vec<Step>, cancel: &CancelToken) {
        execute(&input, cancel);
    }
}

fn run_family(name: &'static str, mix: Mix, ci_cases: u64) {
    let family = Programs { name, mix };
    let findings = fuzz(&family, &Settings::for_family(name, ci_cases));
    assert_no_findings(name, &findings);
}

#[test]
fn panic_fuzz_primitives() {
    run_family("primitives", Mix::Primitives, 150);
}

#[test]
fn panic_fuzz_sweeps() {
    run_family("sweeps", Mix::Sweeps, 30);
}

#[test]
fn panic_fuzz_booleans() {
    run_family("booleans", Mix::Booleans, 15);
}

#[test]
fn panic_fuzz_blends() {
    run_family("blends", Mix::Blends, 15);
}

#[test]
fn panic_fuzz_offsets() {
    run_family("offsets", Mix::Offsets, 4);
}

#[test]
fn panic_fuzz_transforms() {
    run_family("transforms", Mix::Transforms, 15);
}

#[test]
fn panic_fuzz_analysis() {
    run_family("analysis", Mix::Analysis, 25);
}

#[test]
fn panic_fuzz_files() {
    run_family("files", Mix::Files, 25);
}

/// `SOLIDCORE_PANIC_FUZZ_REPRO` takes `<family>:<u64 seed>`; anything else is an error
/// rather than a run that skips every family or falls back to the CI seed set.
#[test]
fn repro_spec_parses() {
    assert_eq!(parse_repro("booleans:12"), ("booleans", 12));
    assert_eq!(parse_repro(" booleans : 12 "), ("booleans", 12));
}

#[test]
#[should_panic(expected = "expected `<family>:<u64 seed>`")]
fn repro_spec_without_a_seed_is_rejected() {
    parse_repro("booleans");
}

#[test]
#[should_panic(expected = "expected `<family>:<u64 seed>`")]
fn repro_spec_with_a_bad_seed_is_rejected() {
    parse_repro("booleans:12x");
}

#[test]
#[should_panic(expected = "expected `<family>:<u64 seed>`")]
fn repro_spec_without_a_family_is_rejected() {
    parse_repro(":12");
}

/// The `u64` settings (`SOLIDCORE_PANIC_FUZZ_CASES`, `_SEED`, `_HANG_SECS`) take decimal
/// digits only; any other form is an error naming the variable and the value, rather
/// than a run that silently falls back to the default.
#[test]
fn u64_setting_parses() {
    assert_eq!(parse_env_u64("SOLIDCORE_PANIC_FUZZ_CASES", "10000"), 10_000);
    assert_eq!(parse_env_u64("SOLIDCORE_PANIC_FUZZ_CASES", " 7 "), 7);
}

#[test]
#[should_panic(expected = "SOLIDCORE_PANIC_FUZZ_CASES=\"10_000\": expected a u64")]
fn u64_setting_with_underscores_is_rejected() {
    parse_env_u64("SOLIDCORE_PANIC_FUZZ_CASES", "10_000");
}

#[test]
#[should_panic(expected = "SOLIDCORE_PANIC_FUZZ_CASES=\"1e4\": expected a u64")]
fn u64_setting_in_exponent_form_is_rejected() {
    parse_env_u64("SOLIDCORE_PANIC_FUZZ_CASES", "1e4");
}

#[test]
#[should_panic(expected = "SOLIDCORE_PANIC_FUZZ_CASES=\"10k\": expected a u64")]
fn u64_setting_with_a_suffix_is_rejected() {
    parse_env_u64("SOLIDCORE_PANIC_FUZZ_CASES", "10k");
}

#[test]
#[should_panic(expected = "SOLIDCORE_PANIC_FUZZ_SEED=\"\": expected a u64")]
fn empty_u64_setting_is_rejected() {
    parse_env_u64("SOLIDCORE_PANIC_FUZZ_SEED", "");
}

/// `SOLIDCORE_PANIC_FUZZ_HANGS` takes `fail`; any other value is an error, not a run that
/// only prints its hangs.
#[test]
fn hangs_setting_parses() {
    assert!(parse_hangs("fail"));
    assert!(parse_hangs(" fail "));
}

#[test]
#[should_panic(expected = "expected `fail` (or unset)")]
fn hangs_setting_true_is_rejected() {
    parse_hangs("true");
}

#[test]
#[should_panic(expected = "expected `fail` (or unset)")]
fn hangs_setting_in_capitals_is_rejected() {
    parse_hangs("FAIL");
}
