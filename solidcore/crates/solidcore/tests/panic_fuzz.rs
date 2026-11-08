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

#[path = "support/brep_pair.rs"]
mod brep_pair;
#[path = "support/offset_fixture.rs"]
mod offset_fixture;
#[path = "support/panic_fuzz.rs"]
mod panic_fuzz;

use std::io::Write as _;

use panic_fuzz::{
    assert_no_findings, fuzz, parse_env_u64, parse_hangs, parse_repro, Family, Rng, Settings,
};
use solidcore::blend::feature_executive::{
    defeature_and_rebuild, defeature_and_rebuild_with_edges, RebuildBrepType,
};
use solidcore::blend::fillet_executive::api as fillet;
use solidcore::blend::fillet_executive::api::FilletSurfaceArg;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::object::ObjectRef;
use solidcore::heal::face_props::HealerOpType;
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::heal::stitch::p1 as stitching;
use solidcore::intersect::api_intersectors as intersectors;
use solidcore::mass::integrator::properties::{brep_mass_properties_default, brep_volume_default};
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::offset::offset_faces::OffsetFacesExecutive;
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as prim;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::query::api_queries as q;
use solidcore::query::brep::BrepIdentity;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep as sweeps;
use solidcore::sweep::api_brep::OffsetCornerType;
use solidcore::sweep::api_surfaces;
use solidcore::topology::api_files::read_part_from_file;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::api_projection as projection;
use solidcore::topology::brep::mirror::mirror;
use solidcore::topology::brep::p1::heal_brep;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::{brep_copy, topology_ref};
use solidcore::topology::brep::p5::remove_faces;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::brep_data_write::{AttributeTable, DATABASE_VERSION};
use solidcore::topology::database_io::{DatabaseWriter, FileType};
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::part_file::{Part, PartEnv};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_scale, api_scale_by_pt, api_transform, TransformObject,
};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, CancelToken, EntityRef, Kernel, Point2, Point3};
use solidcore::{BrepId, Vec3, VertexId};

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
    /// Closest point (`which` 0, 1), relationship (2), distance (3) of bodies `a` and
    /// `b`; `find_faces` (4) and `find_edge` (5) at `point` on `a`.
    BodyQuery {
        a: usize,
        b: usize,
        point: P,
        which: u8,
    },
    /// `boolean_lists` of the bodies `first` and `second`, with the surfaces of the
    /// faces `(body, face, into the first list)`.
    BooleanLists {
        op: i32,
        first: Vec<usize>,
        second: Vec<usize>,
        surfaces: Vec<(usize, usize, bool)>,
    },
    /// `boolean_trees` of the bodies (a `None` slot for each `holes` entry).
    BooleanTrees {
        bodies: Vec<usize>,
        holes: Vec<usize>,
        tree: Vec<i64>,
    },
    /// `non_manifold_sweep` along `dir`, or `non_manifold_rotational_sweep` about
    /// `rotate` (base, axis, angle), of the selected faces, edges, vertices.
    NmSweep {
        body: usize,
        dir: P,
        dist: f64,
        rotate: Option<(P, P, f64)>,
        stitch: bool,
        merge: bool,
        faces: Option<Vec<usize>>,
        edges: Option<Vec<usize>>,
        vertices: Option<Vec<usize>>,
    },
    /// `create_curve_sweep_from_edges` (`edges`) or `_from_faces` of the picks.
    SweepFrom {
        body: usize,
        edges: bool,
        picks: Vec<usize>,
        path: Profile,
        scale: Option<Profile>,
        bits: u8,
    },
    /// `create_skin_from_faces` of `(body, face)` picks.
    SkinFromFaces {
        faces: Vec<(usize, usize)>,
        degree: usize,
    },
    /// `surface_surface_fillet` of face `fa` of `a` and face `fb` of `b` (`free`: bit 0
    /// the first as a free surface, bit 1 the second, bit 2 the second on the first's
    /// body).
    SurfaceFillet {
        a: usize,
        fa: usize,
        b: usize,
        fb: usize,
        free: u8,
        r1: f64,
        r2: f64,
        tolerance: f64,
        xsect: u32,
        mirror: bool,
        complement: bool,
    },
    BlendPrimitive {
        body: usize,
        e1: usize,
        f1: usize,
        e2: usize,
        f2: usize,
        same: bool,
    },
    /// `create_helix` (origin, height, start and end radius, turns, tolerance), then
    /// a pipe of radius `pipe` along it.
    Helix {
        values: [f64; 8],
        right: bool,
        pipe: Option<f64>,
    },
    /// `heal_brep` up to healer step `op` (or `api_heal_brep` for `op > 26`).
    Heal {
        body: usize,
        op: u8,
        subset: Option<(Vec<usize>, Vec<usize>, Vec<usize>)>,
        uv: bool,
    },
    /// The stitch entry points (`kind`).
    Stitch {
        body: usize,
        kind: u8,
        tolerance: f64,
        flags: u8,
        keep: Vec<usize>,
        delete: Vec<usize>,
        samples: u64,
    },
    /// A part file of the bodies and the tree, mutated, read by `read_part_from_file`.
    PartRoundTrip {
        bodies: Vec<usize>,
        tree: Vec<i64>,
        ascii: bool,
        edits: Vec<(f64, u8, u64)>,
    },
    OffsetFaces {
        body: usize,
        faces: Vec<usize>,
        distance: f64,
    },
    /// `defeature_and_rebuild` of the faces, or `_with_edges` of the first face and
    /// the edges.
    Defeature {
        body: usize,
        faces: Vec<usize>,
        edges: Option<Vec<usize>>,
        base: u8,
        feature: u8,
    },
    /// `remove_faces` (the open body is kept, for the stitch and heal steps).
    RemoveFaces {
        body: usize,
        faces: Vec<usize>,
    },
    /// `project_brep_onto_plane` (0), `create_silhouette_curves` (1),
    /// `project_curve_onto_brep` (2) of `curve` along `normal`.
    Project {
        body: usize,
        point: P,
        normal: P,
        which: u8,
        curve: Profile,
    },
    /// `intersect_brep_with_plane` (0), `intersect_breps` of `a` with `b` (1) or
    /// itself (2), `intersect_curve_brep` of `curve` (3).
    Section {
        a: usize,
        b: usize,
        point: P,
        normal: P,
        which: u8,
        curve: Profile,
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
    BodyQueries,
    BooleanSets,
    NmSweeps,
    SurfaceBlends,
    Helix,
    Heal,
    Parts,
    FaceOps,
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
            edits: file_edits(r),
        },
        Mix::BodyQueries => Step::BodyQuery {
            a: body,
            b: r.below(8),
            point: point(r, p),
            which: r.below(6) as u8,
        },
        Mix::BooleanSets => {
            if r.chance(0.5) {
                Step::BooleanLists {
                    op: r.pick(&[0, 1, 2, 3, 4, 0, 1, 3, -1, 5, i32::MAX, i32::MIN]),
                    first: picks(r, 3),
                    second: picks(r, 3),
                    surfaces: (0..r.below(3))
                        .map(|_| (r.below(8), r.below(64), r.chance(0.5)))
                        .collect(),
                }
            } else {
                Step::BooleanTrees {
                    bodies: picks(r, 4),
                    holes: picks(r, 1),
                    tree: tree(r),
                }
            }
        }
        Mix::NmSweeps => match r.below(4) {
            0 | 1 => {
                let sel = |r: &mut Rng| r.chance(0.5).then(|| picks(r, 3));
                Step::NmSweep {
                    body,
                    dir: direction(r, p),
                    dist: size(r, p),
                    rotate: r
                        .chance(0.4)
                        .then(|| (point(r, p), direction(r, p), angle(r, p))),
                    stitch: r.chance(0.5),
                    merge: r.chance(0.5),
                    faces: sel(r),
                    edges: sel(r),
                    vertices: sel(r),
                }
            }
            2 => Step::SweepFrom {
                body,
                edges: r.chance(0.5),
                picks: picks(r, 4),
                path: profile(r, p),
                scale: r.chance(0.2).then(|| profile(r, p)),
                bits: r.next_u64() as u8,
            },
            _ => Step::SkinFromFaces {
                faces: (0..r.below(4)).map(|_| (r.below(8), r.below(64))).collect(),
                degree: r.pick(&[0, 1, 2, 3, 3, 5, 99, usize::MAX]),
            },
        },
        Mix::SurfaceBlends => {
            if r.chance(0.6) {
                Step::SurfaceFillet {
                    a: body,
                    fa: r.below(64),
                    b: r.below(8),
                    fb: r.below(64),
                    free: r.below(8) as u8,
                    r1: r.hostile(p, -2.0, 2.0),
                    r2: r.hostile(p, -2.0, 2.0),
                    tolerance: r.hostile(p, 1e-4, 1e-2),
                    xsect: r.pick(&[0, 1, 2, 3, u32::MAX]),
                    mirror: r.chance(0.3),
                    complement: r.chance(0.3),
                }
            } else {
                Step::BlendPrimitive {
                    body,
                    e1: r.below(64),
                    f1: r.below(64),
                    e2: r.below(64),
                    f2: r.below(64),
                    same: r.chance(0.5),
                }
            }
        }
        Mix::Helix => {
            let o = point(r, p);
            Step::Helix {
                values: [
                    o[0],
                    o[1],
                    o[2],
                    size(r, p),
                    size(r, p),
                    size(r, p),
                    r.hostile(p, 0.1, 12.0),
                    r.hostile(p, 1e-4, 1e-2),
                ],
                right: r.chance(0.5),
                pipe: r.chance(0.2).then(|| r.hostile(p, 0.05, 0.5)),
            }
        }
        Mix::Heal => {
            if r.chance(0.4) {
                Step::Heal {
                    body,
                    op: r.pick(&[26, 26, 5, 7, 9, 12, 16, 20, 23, 25, 27]),
                    subset: r
                        .chance(0.3)
                        .then(|| (picks(r, 3), picks(r, 3), picks(r, 3))),
                    uv: r.chance(0.3),
                }
            } else {
                Step::Stitch {
                    body,
                    kind: r.below(6) as u8,
                    tolerance: r.hostile(p, 1e-6, 1e-1),
                    flags: r.below(16) as u8,
                    keep: picks(r, 3),
                    delete: picks(r, 3),
                    samples: r.pick(&[0, 1, 4, 64, u64::MAX]),
                }
            }
        }
        Mix::Parts => Step::PartRoundTrip {
            bodies: picks(r, 3),
            tree: tree(r),
            ascii: r.chance(0.5),
            edits: file_edits(r),
        },
        Mix::FaceOps => match r.below(5) {
            0 => Step::OffsetFaces {
                body,
                faces: picks(r, 3),
                distance: r.hostile(p, -1.0, 1.0),
            },
            1 => Step::Defeature {
                body,
                faces: picks(r, 3),
                edges: r.chance(0.3).then(|| picks(r, 4)),
                base: r.below(4) as u8,
                feature: r.below(4) as u8,
            },
            2 => Step::RemoveFaces {
                body,
                faces: picks(r, 2),
            },
            3 => Step::Project {
                body,
                point: point(r, p),
                normal: direction(r, p),
                which: r.below(3) as u8,
                curve: profile(r, p),
            },
            _ => Step::Section {
                a: body,
                b: r.below(8),
                point: point(r, p),
                normal: direction(r, p),
                which: r.below(4) as u8,
                curve: profile(r, p),
            },
        },
    }
}

fn file_edits(r: &mut Rng) -> Vec<(f64, u8, u64)> {
    (0..r.below(4))
        .map(|_| (r.unit(), r.below(6) as u8, r.next_u64()))
        .collect()
}

/// A postfix Boolean tree: mostly well formed (leaves then operators), sometimes
/// random nodes, out-of-range leaves and operators.
fn tree(r: &mut Rng) -> Vec<i64> {
    if r.chance(0.7) {
        let leaves = 1 + r.below(4);
        let mut t: Vec<i64> = (0..leaves).map(|_| r.below(5) as i64).collect();
        for _ in 1..leaves {
            t.push(-(1 + r.below(6) as i64));
        }
        t
    } else {
        (0..r.below(6))
            .map(|_| r.pick(&[0, 1, 2, 7, -1, -2, -3, -4, -5, -6, -7, i64::MIN, i64::MAX]))
            .collect()
    }
}

/// A program: one to three body-making steps, then (except for the body-making
/// families) one to three operations, some of them on earlier results.
fn program(r: &mut Rng, mix: Mix) -> Vec<Step> {
    // most programs keep most values sane so they get deep into an operation
    let p = r.pick(&[0.0, 0.05, 0.15, 0.4]);
    match mix {
        Mix::Primitives | Mix::Sweeps | Mix::Helix => {
            (0..=r.below(2)).map(|_| op_step(r, mix, p)).collect()
        }
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

fn picked_vertices(b: &Brep, picks: &[usize]) -> Vec<VertexId> {
    let all: Vec<VertexId> = b.vertices().ids().collect();
    picks.iter().filter_map(|&i| nth(&all, i)).collect()
}

fn pick_face(b: &Brep, i: usize) -> Option<FaceId> {
    nth(&q::get_faces(b).unwrap_or_default(), i)
}

fn pick_edge(b: &Brep, i: usize) -> Option<EdgeId> {
    nth(&q::get_edges(b).unwrap_or_default(), i)
}

fn face_surface(b: &Brep, i: usize) -> Option<Surface> {
    let f = pick_face(b, i)?;
    face_p2::surface(b, f).ok().flatten().cloned()
}

fn vec3(p: P) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The body-to-body and point queries.
fn body_query(k: &mut Kernel, a: &Brep, b: &Brep, point: P, which: u8) {
    // read-only stand-ins that keep their geometry's owners (a `Brep` clone has none)
    let (Ok(mut a), Ok(mut b)) = (a.clone_standing_for_self(), b.clone_standing_for_self()) else {
        return;
    };
    if which >= 4 {
        if which == 4 {
            let _ = q::find_faces(
                k,
                &mut a,
                BrepId::from_index(0),
                pt(point),
                &arena_identity,
                &|_| None,
            );
        } else {
            let _ = q::find_edge(k, &a, pt(point));
        }
        return;
    }
    let proven = [
        BrepIdentity::of_brep_with_id(BrepId::from_index(0), &a),
        BrepIdentity::of_brep_with_id(BrepId::from_index(1), &b),
    ];
    let identities = [proven[0].identity(), proven[1].identity()];
    let mut empty = Brep::new();
    let second = if which < 2 { &mut empty } else { &mut b };
    let Ok(objects) = brep_pair::Pair::new(k, [&mut a, second], &identities) else {
        return;
    };
    let h = [
        brep_pair::Object(0, CacheObject::Brep),
        brep_pair::Object(1, CacheObject::Brep),
    ];
    match which {
        0 => {
            let _ = q::brep_closest_point(k, &objects, h[0], pt(point));
        }
        1 => {
            let _ = q::brep_closest_point_all(k, &objects, h[0], pt(point));
        }
        _ => {
            // the solve on the live B-reps, then the classification on them (as a
            // caller holding the B-reps mutably does; `brep_relationship` and
            // `brep_distance` are the two in one)
            let Ok(solution) = q::brep_relationship_solve(k, &objects, h[0], h[1]) else {
                return;
            };
            let [l0, l1] = &objects.lives;
            let _ = l0.with_brep(|a| {
                l1.with_brep(|b| {
                    if which == 2 {
                        q::brep_relationship_classify(k, &objects, &solution, h, a, b).map(|_| ())
                    } else {
                        let (a, b) = (
                            q::BrepRef {
                                brep: a,
                                handle: h[0],
                            },
                            q::BrepRef {
                                brep: b,
                                handle: h[1],
                            },
                        );
                        q::brep_distance(k, &objects, a, b).map(|_| ())
                    }
                })
            });
        }
    }
}

fn helix(k: &mut Kernel, v: &[f64; 8], right: bool, pipe: Option<f64>) -> Option<Brep> {
    let c = curves::create_helix(&curves::HelixParams {
        origin: pt([v[0], v[1], v[2]]),
        height: v[3],
        radius_start: v[4],
        radius_end: v[5],
        num_turns: v[6],
        right_handed: right,
        tolerance: v[7],
    })
    .ok()?;
    let path = Curve::from(c);
    sweeps::create_pipe_sweep(k, pipe?, Some(&path), true).ok()
}

fn healer_op(op: u8) -> Option<HealerOpType> {
    HealerOpType::from_code(i64::from(op))
}

fn stitch(k: &mut Kernel, b: &mut Brep, s: &Step) {
    let Step::Stitch {
        kind,
        tolerance,
        flags,
        keep,
        delete,
        samples,
        ..
    } = s
    else {
        return;
    };
    match kind {
        0 => {
            let _ = stitching::api_stitch(k, Some(b));
        }
        1 => {
            let _ = stitching::api_stitch_into_solid(k, Some(b));
        }
        2 => {
            let _ = stitching::api_stitch_into_shell(k, Some(b), flags & 1 != 0, *tolerance);
        }
        3 => {
            let options = stitching::AdvancedStitchOptions {
                squeeze_small_edges: flags & 1 != 0,
                split_edges_with_vertices: flags & 2 != 0,
                making_manifold_solid: flags & 4 != 0,
                remove_laminar_slivers: flags & 8 != 0,
            };
            let _ = stitching::api_stitch_advanced(k, Some(b), *tolerance, options);
        }
        4 => {
            let start = picked_faces(b, keep).first().copied();
            let _ = stitching::api_unify_normals(k, Some(b), start, *samples);
        }
        _ => {
            let (keep, delete) = (picked_faces(b, keep), picked_faces(b, delete));
            let _ = stitching::api_simple_face_stitch(k, Some(b), &keep, &delete);
        }
    }
}

/// The B-rep section of a part file: `b` as `SmBrepData` written with its attribute
/// indices (`WriteToDB(.., TRUE)`), its surfaces' NURBs made first.
fn part_brep(k: &Kernel, b: &Brep, ascii: bool) -> Option<Vec<u8>> {
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
    let file_type = if ascii {
        FileType::Ascii
    } else {
        FileType::Binary
    };
    let mut all = Vec::new();
    let mut data = BrepData::from_brep(b, &env, &mut all).ok()?;
    // (makes the missing NURBs)
    data.write_to_file_with_nurbs(k, file_type).ok()?;
    let mut map = std::collections::BTreeSet::new();
    let mut attrs = AttributeTable {
        store: &store,
        all: &mut all,
        map: &mut map,
    };
    let mut db = DatabaseWriter::open_for_write(file_type);
    data.write_to_db(DATABASE_VERSION, &mut attrs, &mut db, true)
        .ok()?;
    Some(db.into_bytes())
}

/// A part file of `breps` and `tree` (no stand-alone curves or surfaces), in the
/// layout `read_part_from_file` reads.
fn part_file(k: &Kernel, breps: &[&Brep], tree: &[i64], ascii: bool) -> Option<Vec<u8>> {
    let version = DATABASE_VERSION;
    let bodies = breps
        .iter()
        .map(|b| part_brep(k, b, ascii))
        .collect::<Option<Vec<_>>>()?;
    let mut out = Vec::new();
    if ascii {
        let _ = write!(
            out,
            "//[Output Summary]\n//   (1) {} Curves\n//   (2) 0 Surfaces\n\
             //   (3) {} Boolean Tree Nodes\n//   (4) {} Breps\n\n// Boolean Trees\n",
            version * 1_000_000,
            tree.len(),
            breps.len()
        );
        for node in tree {
            let _ = writeln!(out, "{node}");
        }
        out.extend_from_slice(b"\n\n");
        for (i, body) in bodies.iter().enumerate() {
            let _ = write!(out, "\n\n// [Brep #{i}]\n");
            out.extend_from_slice(body);
        }
        out.extend_from_slice(b"\n0 // Total number of attributes\n");
    } else {
        for count in [
            version * 1_000_000,
            0,
            tree.len() as u64,
            breps.len() as u64,
        ] {
            out.extend_from_slice(&count.to_le_bytes());
        }
        for node in tree {
            out.extend_from_slice(&node.to_le_bytes());
        }
        for body in &bodies {
            out.extend_from_slice(body);
        }
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    Some(out)
}

fn part_round_trip(
    k: &mut Kernel,
    breps: &[&Brep],
    tree: &[i64],
    ascii: bool,
    edits: &[(f64, u8, u64)],
) {
    let Some(mut bytes) = part_file(k, breps, tree, ascii) else {
        return;
    };
    mutate(&mut bytes, edits, ascii);
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let key_of = |i: usize, e: EntityRef| caller_named_in(i, e);
    let mut env = PartEnv {
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut part = Part {
        curves: Vec::new(),
        surfaces: Vec::new(),
        boolean_tree_nodes: Vec::new(),
        breps: Vec::new(),
    };
    if read_part_from_file(k, Some(bytes), ascii, &mut part, &mut env).is_ok() {
        for b in &mut part.breps {
            b.refresh_cached_topology();
            let _ = q::brep_bounding_box(k, b, false);
            // (a coarse volume: the read-back bodies are whole, and the part's tree
            // runs on them below)
            let _ = brep_volume_default(k, b, 1e-2);
        }
        let mut slots: Vec<Option<Brep>> = part.breps.into_iter().map(Some).collect();
        let _ = boolean::boolean_trees(k, &mut slots, &part.boolean_tree_nodes);
    }
}

fn caller_named_in(i: usize, entity: EntityRef) -> ObjectRef {
    let r = caller_named(entity);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(r.key.0 + (i as u64) * 100_000_000_000),
    }
}

fn rebuild_type(t: u8) -> RebuildBrepType {
    [
        RebuildBrepType::None,
        RebuildBrepType::PlanarCap,
        RebuildBrepType::ExtendSurface,
        RebuildBrepType::FromOtherBrep,
    ][usize::from(t) % 4]
}

/// The steps of the families added after slice 1 that need a body (`bodies` is not
/// empty); `Some` with a body to keep.
#[allow(clippy::too_many_lines)] // one arm per step
fn wide_step(k: &mut Kernel, step: &Step, bodies: &[Brep]) -> Option<Brep> {
    let n = bodies.len();
    // an independent copy that owns its geometry (a `Brep` clone owns none); a copy
    // that fails skips the step
    let body = |k: &mut Kernel, i: usize| brep_copy(k, &bodies[i % n]).ok();
    match step {
        Step::BodyQuery { a, b, point, which } => {
            body_query(k, &bodies[a % n], &bodies[b % n], *point, *which);
            None
        }
        Step::BooleanLists {
            op,
            first,
            second,
            surfaces,
        } => {
            let l1: Vec<Brep> = first.iter().map(|&i| body(k, i)).collect::<Option<_>>()?;
            let l2: Vec<Brep> = second.iter().map(|&i| body(k, i)).collect::<Option<_>>()?;
            let (mut s1, mut s2) = (Vec::new(), Vec::new());
            for &(b, f, into_first) in surfaces {
                if let Some(s) = face_surface(&bodies[b % n], f) {
                    if into_first {
                        s1.push(s);
                    } else {
                        s2.push(s);
                    }
                }
            }
            boolean::boolean_lists(k, &l1, &s1, &l2, &s2, *op)
                .ok()
                .and_then(|r| r.breps.into_iter().next())
        }
        Step::BooleanTrees {
            bodies: which,
            holes,
            tree,
        } => {
            let mut slots: Vec<Option<Brep>> = which
                .iter()
                .map(|&i| body(k, i).map(Some))
                .collect::<Option<_>>()?;
            for &h in holes {
                if !slots.is_empty() {
                    let i = h % slots.len();
                    slots[i] = None;
                }
            }
            boolean::boolean_trees(k, &mut slots, tree).ok()?;
            slots.into_iter().flatten().next()
        }
        Step::NmSweep {
            body: i,
            dir,
            dist,
            rotate,
            stitch,
            merge,
            faces,
            edges,
            vertices,
        } => {
            let mut b = body(k, *i)?;
            let f: Option<Vec<Option<FaceId>>> = faces
                .as_ref()
                .map(|p| picked_faces(&b, p).into_iter().map(Some).collect());
            let e: Option<Vec<Option<EdgeId>>> = edges
                .as_ref()
                .map(|p| picked_edges(&b, p).into_iter().map(Some).collect());
            let v: Option<Vec<Option<VertexId>>> = vertices
                .as_ref()
                .map(|p| picked_vertices(&b, p).into_iter().map(Some).collect());
            let selection = sweeps::SweepSelection {
                faces: f.as_deref(),
                edges: e.as_deref(),
                vertices: v.as_deref(),
            };
            let r = match rotate {
                None => sweeps::non_manifold_sweep(
                    k,
                    &mut b,
                    None,
                    vec3(*dir),
                    *dist,
                    *stitch,
                    *merge,
                    selection,
                ),
                Some((base, axis, angle)) => sweeps::non_manifold_rotational_sweep(
                    k,
                    &mut b,
                    Some(sweeps::RegionBrep::Swept),
                    pt(*base),
                    vec3(*axis),
                    *angle,
                    *stitch,
                    *merge,
                    selection,
                ),
            };
            r.ok().map(|()| b)
        }
        Step::SweepFrom {
            body: i,
            edges,
            picks,
            path,
            scale,
            bits,
        } => {
            let b = body(k, *i)?;
            let path = curves_of(k, path)?;
            let scale = match scale {
                Some(s) => Some(curves_of(k, s)?),
                None => None,
            };
            let options = SweepOptions {
                cap_ends: bits & 1 != 0,
                translational_sweep: bits & 2 != 0,
                move_profile_center_to_path: bits & 4 != 0,
                orient_profile_perpendicular_to_path: bits & 8 != 0,
                move_to_profile: bits & 16 != 0,
                profile_at_path_start: bits & 32 != 0,
            };
            let sc0 = scale.as_ref().and_then(|v| v.first());
            let sc1 = scale.as_ref().and_then(|v| v.get(1));
            let options = (bits & 64 == 0).then_some(&options);
            let r = if *edges {
                let e: Vec<(&Brep, EdgeId)> = picked_edges(&b, picks)
                    .into_iter()
                    .map(|e| (&b, e))
                    .collect();
                sweeps::create_curve_sweep_from_edges(k, &e, path.first(), sc0, sc1, options)
            } else {
                let f: Vec<(&Brep, FaceId)> = picked_faces(&b, picks)
                    .into_iter()
                    .map(|f| (&b, f))
                    .collect();
                sweeps::create_curve_sweep_from_faces(k, &f, path.first(), sc0, sc1, options)
            };
            r.ok().map(|s| s.brep)
        }
        Step::SkinFromFaces { faces, degree } => {
            let owners: Vec<Brep> = faces
                .iter()
                .map(|&(b, _)| body(k, b))
                .collect::<Option<_>>()?;
            let f: Vec<(&Brep, FaceId)> = owners
                .iter()
                .zip(faces)
                .filter_map(|(b, &(_, f))| Some((b, pick_face(b, f)?)))
                .collect();
            api_surfaces::create_skin_from_faces(k, &f, *degree)
                .ok()
                .map(|s| s.brep)
        }
        Step::SurfaceFillet {
            a,
            fa,
            b,
            fb,
            free,
            r1,
            r2,
            tolerance,
            xsect,
            mirror,
            complement,
        } => {
            let (mut ba, mut bb) = (body(k, *a)?, body(k, *b)?);
            let (sa, sb) = (face_surface(&ba, *fa)?, face_surface(&bb, *fb)?);
            let (Some(face_a), Some(face_b)) = (pick_face(&ba, *fa), pick_face(&bb, *fb)) else {
                return None;
            };
            let first = if free & 1 != 0 {
                FilletSurfaceArg::Free(&sa)
            } else {
                FilletSurfaceArg::Face {
                    body: &mut ba,
                    face: face_a,
                }
            };
            let second = if free & 4 != 0 {
                // a face of the first body (only with the first on a body)
                FilletSurfaceArg::FirstBodyFace(face_b)
            } else if free & 2 != 0 {
                FilletSurfaceArg::Free(&sb)
            } else {
                FilletSurfaceArg::Face {
                    body: &mut bb,
                    face: face_b,
                }
            };
            fillet::surface_surface_fillet(
                k,
                first,
                second,
                *r1,
                *r2,
                *tolerance,
                *xsect,
                *mirror,
                *complement,
            )
            .ok()
            .flatten()
        }
        Step::BlendPrimitive {
            body: i,
            e1,
            f1,
            e2,
            f2,
            same,
        } => {
            let mut b = body(k, *i)?;
            let (Some(e1), Some(f1), Some(e2), Some(f2)) = (
                pick_edge(&b, *e1),
                pick_face(&b, *f1),
                pick_edge(&b, *e2),
                pick_face(&b, *f2),
            ) else {
                return None;
            };
            prim::create_blend_primitive(k, &mut b, e1, f1, e2, f2, *same)
                .ok()
                .map(|_| b)
        }
        Step::Heal {
            body: i,
            op,
            subset,
            uv,
        } => {
            let mut b = body(k, *i)?;
            let r = match healer_op(*op) {
                None => api_heal_brep(k, Some(&mut b)),
                Some(op) => {
                    let lists = subset.as_ref().map(|(v, e, f)| {
                        (
                            picked_vertices(&b, v),
                            picked_edges(&b, e),
                            picked_faces(&b, f),
                        )
                    });
                    let (v, e, f) = match &lists {
                        Some((v, e, f)) => (Some(&v[..]), Some(&e[..]), Some(&f[..])),
                        None => (None, None, None),
                    };
                    heal_brep(k, &mut b, op, v, e, f, *uv)
                }
            };
            r.ok().map(|()| b)
        }
        Step::Stitch { body: i, .. } => {
            let mut b = body(k, *i)?;
            stitch(k, &mut b, step);
            Some(b)
        }
        Step::PartRoundTrip {
            bodies: which,
            tree,
            ascii,
            edits,
        } => {
            // (the bodies themselves, as for `FileRoundTrip`)
            let breps: Vec<&Brep> = which.iter().map(|&i| &bodies[i % n]).collect();
            part_round_trip(k, &breps, tree, *ascii, edits);
            None
        }
        Step::OffsetFaces {
            body: i,
            faces,
            distance,
        } => {
            let mut b = body(k, *i)?;
            let f = picked_faces(&b, faces);
            OffsetFacesExecutive::new()
                .offset_faces(k, &mut b, &f, *distance)
                .ok()
                .map(|()| b)
        }
        Step::Defeature {
            body: i,
            faces,
            edges,
            base,
            feature,
        } => {
            let mut b = body(k, *i)?;
            let f = picked_faces(&b, faces);
            let (base, feature) = (rebuild_type(*base), rebuild_type(*feature));
            let r = match edges {
                Some(e) => {
                    let e = picked_edges(&b, e);
                    let face = f.first().copied()?;
                    defeature_and_rebuild_with_edges(k, &mut b, face, &e, base, feature)
                }
                None => defeature_and_rebuild(k, &mut b, &f, base, feature),
            };
            r.ok().map(|_| b)
        }
        Step::RemoveFaces { body: i, faces } => {
            let mut b = body(k, *i)?;
            let f = picked_faces(&b, faces);
            remove_faces(k, &mut b, true, &f).ok().map(|()| b)
        }
        Step::Project {
            body: i,
            point,
            normal,
            which,
            curve,
        } => {
            let mut b = body(k, *i)?;
            let mut out = Vec::new();
            match which {
                0 => {
                    let _ = projection::project_brep_onto_plane(
                        k,
                        Some(&b),
                        vec3(*point),
                        vec3(*normal),
                        &mut out,
                    );
                }
                1 => {
                    let _ = projection::create_silhouette_curves(
                        k,
                        Some(&b),
                        vec3(*point),
                        vec3(*normal),
                        &mut out,
                    );
                }
                _ => {
                    let c = bspline_of(k, curve);
                    let _ = projection::project_curve_onto_brep(
                        k,
                        Some(&mut b),
                        c.as_ref(),
                        vec3(*normal),
                        &mut out,
                    );
                }
            }
            None
        }
        Step::Section {
            a,
            b,
            point,
            normal,
            which,
            curve,
        } => {
            let mut x = body(k, *a)?;
            match which {
                0 => {
                    let _ = intersectors::intersect_brep_with_plane(
                        k,
                        &mut x,
                        pt(*point),
                        vec3(*normal),
                    );
                }
                1 => {
                    let mut y = body(k, *b)?;
                    let _ = intersectors::intersect_breps(
                        k,
                        intersectors::BrepOperands::Two(&mut x, &mut y),
                        true,
                    );
                }
                2 => {
                    let _ = intersectors::intersect_breps(
                        k,
                        intersectors::BrepOperands::Same(&mut x),
                        false,
                    );
                }
                _ => {
                    if let Some(c) = curves_of(k, curve).and_then(|c| c.into_iter().next()) {
                        let _ = intersectors::intersect_curve_brep(k, &c, &mut x);
                    }
                }
            }
            None
        }
        _ => None,
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
                // (the body itself: a `Brep::clone` drops its face surfaces' owners, so
                // the writer would leave its surfaces out)
                file_round_trip(k, &bodies[body % n], *ascii, edits);
                None
            }
            Step::Helix {
                values,
                right,
                pipe,
            } => helix(k, values, *right, *pipe),
            _ if n > 0 => wide_step(k, step, &bodies),
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

#[test]
fn panic_fuzz_body_queries() {
    run_family("body_queries", Mix::BodyQueries, 15);
}

#[test]
fn panic_fuzz_boolean_sets() {
    run_family("boolean_sets", Mix::BooleanSets, 4);
}

#[test]
fn panic_fuzz_nm_sweeps() {
    // (the fifth case, 15137747025202472589, a curve sweep from edges along an ellipse,
    // takes 10 s optimised and past the hang limit in a debug build; the slow cases the
    // fuzz finds are tracked in #2756)
    run_family("nm_sweeps", Mix::NmSweeps, 4);
}

#[test]
fn panic_fuzz_surface_blends() {
    run_family("surface_blends", Mix::SurfaceBlends, 10);
}

#[test]
fn panic_fuzz_helix() {
    run_family("helix", Mix::Helix, 40);
}

#[test]
fn panic_fuzz_heal() {
    run_family("heal", Mix::Heal, 15);
}

#[test]
fn panic_fuzz_parts() {
    // (the seventh case, a binary part of a swung body and a torus read back, took past
    // 20 s in a debug build on Brep clones; see #2756 for the slow cases)
    run_family("parts", Mix::Parts, 6);
}

#[test]
fn panic_fuzz_face_ops() {
    run_family("face_ops", Mix::FaceOps, 10);
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
