//! WR-698 (#2273, the fork's 4a10ed8): with `fillet_round_as_definition` the G^n section
//! generator's round is its definition,
//! a procedural blend (`SmProceduralBlend`) on the certified net it built: its faces,
//! the ball's offsets and sides, the section, the centre line as the leader, the feet's
//! uv curves as the seeds and the certified bound. A face that is itself a blend by its
//! definition is refused (its net is not the surface the feet are solved on).
//!
//! Expected values: fork 3bc54a4 (`SMLib` and `SM_API` only; 4a10ed8 gave the same to
//! 1e-9 on the planes, and its own pipe values before 3bc54a4's jets), built locally with the
//! harness' flags and driven through `SmApiFilletEdges` on the same bodies as
//! `wr685_gn_equal_area_per_station.rs`: each blend face's type, its points at 16 domain
//! fractions (`EvaluatePoint`, the definition's), within 1e-9 relative (an absolute
//! floor of 1e-9 times the body's size for coordinates the fork puts near 0), and the
//! body's volume within 1e-7 (the volume integrator's tolerance). The volume
//! integrates the definition's points, not the net's (the fork at this commit has no net
//! scope in the tessellator yet). The fork's faces run 6.1e-4 (the chamfered box's 40)
//! and 9.1e-4 (the pipe's 60) past the edge's end, on a sliver span its shrink kept past
//! the knot; here the shrink ends on the knot (WR-702), so each face runs along the edge
//! from 0 to its length and a point's x is its domain fraction times that length. The
//! fork's y and z are kept.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::{BlendCounts, PROCEDURAL_BLEND_TYPE};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel(definition: bool) -> Kernel {
    let mut fixes = UpstreamFixes::default();
    fixes.fillet_round_as_definition = definition;
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    volume_at(k, b, 1e-7)
}

/// `SmApiBrepComputeVolume` at `accuracy`.
fn volume_at(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

/// The accuracy of a volume compared only with another's, the body as built and as
/// reopened: both integrate S at the same accuracy, so they agree to rounding at any
/// accuracy where the reopened S is the built one's, and differ by the volume between the
/// two where it is not.
const SAME_VOLUME_ACCURACY: f64 = 1e-5;

/// The fork's values for one body.
struct Fork {
    volume: f64,
    /// Each blend face's points at the domain fractions (u, v), u and v each 0, 0.3,
    /// 0.5, 1 (u outer) and 0, 0.25, 0.7, 1.
    faces: &'static [&'static [[f64; 3]]],
}

/// The faces whose surface is a blend (any kind with the G^n section's degree 7 in v).
fn blend_faces(b: &Brep) -> Vec<Surface> {
    let mut out = Vec::new();
    for &f in b.face_list() {
        let Some(s) = b.face(f).unwrap().surface.clone() else {
            continue;
        };
        let net = match s.kind() {
            SurfaceKind::BSpline(bs) => bs,
            SurfaceKind::ProceduralBlend(pb) => pb.net(),
            _ => continue,
        };
        if net.degree(solidcore::nurbs::b_spline_surface::SurfParamType::V) == 7 {
            out.push(s);
        }
    }
    out
}

fn close(a: f64, b: f64, size: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()) || (a - b).abs() <= 1e-9 * size
}

fn check(what: &str, k: &Kernel, b: &mut Brep, want: &Fork, size: f64) {
    let faces = blend_faces(b);
    assert_eq!(faces.len(), want.faces.len(), "{what}: blend faces");
    let fr = [0.0, 0.3, 0.5, 1.0];
    let fc = [0.0, 0.25, 0.7, 1.0];
    for wf in want.faces {
        // the faces may come in another order: match by the first point
        let s = faces
            .iter()
            .find(|s| {
                let d = s.natural_uv_domain_with_nurb(k).unwrap();
                let p = s.evaluate_point(k, d.evaluate(0.0, 0.0)).unwrap();
                [p.x, p.y, p.z]
                    .iter()
                    .zip(wf[0])
                    .all(|(a, b)| close(*a, b, size))
            })
            .unwrap_or_else(|| panic!("{what}: no face starts at {:?}", wf[0]));
        assert_eq!(
            s.type_code(),
            PROCEDURAL_BLEND_TYPE,
            "{what}: a blend by its definition"
        );
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        let mut i = 0;
        for a in fr {
            for c in fc {
                let p = s.evaluate_point(k, d.evaluate(a, c)).unwrap();
                let q = wf[i];
                assert!(
                    [p.x, p.y, p.z]
                        .iter()
                        .zip(q)
                        .all(|(a, b)| close(*a, b, size)),
                    "{what}: point {i} at ({a}, {c}): {p:?}, the fork's {q:?}"
                );
                i += 1;
            }
        }
    }
    let v = volume_of(k, b);
    assert!(
        (v - want.volume).abs() <= 1e-7 * want.volume,
        "{what}: volume {v}, the fork's {}",
        want.volume
    );
}

/// Two edges of different wedges after the box's top back edge is chamfered 5 (the
/// chamfer's top edge and the box's bottom front edge), rounded r 2 at the equal area
/// default; and, without the definition flag, the same rounds stay B-splines.
#[test]
fn a_gn_round_is_its_definition() {
    const FORK: Fork = Fork {
        volume: 23462.221395287153,
        faces: &[
            &[
                [0.0, 24.17157287525382, 20.0],
                [-6.58972293617564e-18, 24.38297445045419, 19.99409258685693],
                [
                    -8.029832232476577e-16,
                    25.378086754236588,
                    19.601604593492233,
                ],
                [
                    -8.881682553047583e-16,
                    25.58578643762691,
                    19.414213562373096,
                ],
                [12.0, 24.171572875253823, 20.000000000000004],
                [12.0, 24.382974450454192, 19.994092586856933],
                [12.0, 25.378086754236588, 19.601604593492226],
                [12.0, 25.58578643762691, 19.41421356237309],
                [20.0, 24.171572875253837, 20.0],
                [20.0, 24.382974450454206, 19.99409258685693],
                [20.0, 25.378086754236595, 19.601604593492215],
                [20.0, 25.58578643762692, 19.414213562373078],
                [40.0, 24.171572875253833, 20.000000000000004],
                [40.0, 24.382974450454203, 19.994092586856933],
                [40.0, 25.378086754236595, 19.601604593492223],
                [40.0, 25.58578643762692, 19.414213562373085],
            ],
            &[
                [0.0, 1.9999999999999987, 0.0],
                [0.0, 1.5826788687379034, 0.024074835266159174],
                [0.0, 0.05768975730114455, 1.4473588091098626],
                [0.0, 0.0, 2.0],
                [12.0, 1.9999999999999987, 0.0],
                [12.0, 1.5826788687379034, 0.024074835266159215],
                [12.0, 0.05768975730114459, 1.4473588091098637],
                [12.0, 0.0, 2.0000000000000004],
                [20.0, 1.9999999999999982, 0.0],
                [20.0, 1.582678868737903, 0.02407483526615916],
                [20.0, 0.05768975730114452, 1.4473588091098621],
                [20.0, 0.0, 1.9999999999999996],
                [40.0, 1.9999999999999984, 0.0],
                [40.0, 1.5826788687379032, 0.024074835266159215],
                [40.0, 0.05768975730114459, 1.4473588091098637],
                [40.0, 0.0, 2.0000000000000004],
            ],
        ],
    };
    for definition in [true, false] {
        let mut k = kernel(definition);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let c = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[c], 5.0, 0, 2, 0.0).unwrap();
        let both = [
            pick(&k, &b, [20.0, 25.0, 20.0]),
            pick(&k, &b, [20.0, 0.0, 0.0]),
        ];
        fillet_edges(&mut k, &mut b, &both, 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
        if definition {
            check("chamfered box", &k, &mut b, &FORK, 40.0);
        } else {
            let faces = blend_faces(&b);
            assert_eq!(faces.len(), 2);
            assert!(faces
                .iter()
                .all(|s| matches!(s.kind(), SurfaceKind::BSpline(_))));
        }
    }
}

/// The fork's pipe on a plate: a plane and a cylinder, the round r 2 at the equal area
/// default, its definition's points on the cylinder.
#[test]
fn a_pipe_root_round_is_its_definition() {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::math::Vec3;
    use solidcore::primitive::api_primitives::create_cylinder;
    const FORK: Fork = Fork {
        volume: 40307.88563787835,
        faces: &[&[
            [0.0, 29.428090415820638, 12.666666666666668],
            [4.733382275710106e-19, 29.31171126155042, 12.247007081058515],
            [
                2.6124984748312377e-17,
                30.646012679999725,
                10.106338480640954,
            ],
            [3.4694469519536167e-17, 31.313708498984766, 10.0],
            [18.0, 29.428090415820638, 12.66666666666667],
            [18.0, 29.31171126155042, 12.247007081058516],
            [18.0, 30.64601267999972, 10.106338480640954],
            [18.0, 31.313708498984763, 10.0],
            [30.0, 29.428090415820627, 12.666666666666664],
            [30.0, 29.311711261550418, 12.247007081058515],
            [30.0, 30.64601267999972, 10.106338480640956],
            [30.0, 31.313708498984756, 9.999999999999998],
            [60.0, 29.428090415820634, 12.66666666666667],
            [60.0, 29.311711261550418, 12.247007081058516],
            [60.0, 30.646012679999718, 10.106338480640954],
            [60.0, 31.31370849898476, 10.0],
        ]],
    };
    let mut k = kernel(true);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(&mut k, &mut pipe, &pl, None, false).unwrap();
    let mut b = boolean_union(&mut k, plate, pipe).unwrap();
    let e = pick(&k, &b, [30.0, 28.0, 10.0]);
    fillet_edges(&mut k, &mut b, &[e], 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
    check("pipe", &k, &mut b, &FORK, 60.0);
}

/// The chamfered box with its two G^n rounds by definition (`a_gn_round_is_its_definition`).
fn chamfered_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let c = pick(k, &b, [20.0, 30.0, 20.0]);
    fillet_edges(k, &mut b, &[c], 5.0, 0, 2, 0.0).unwrap();
    let both = [
        pick(k, &b, [20.0, 25.0, 20.0]),
        pick(k, &b, [20.0, 0.0, 0.0]),
    ];
    fillet_edges(k, &mut b, &both, 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
    b
}

/// #2388: a body whose G^n rounds are rebuilt from their records (the reopened body) is
/// certified again face by face; one whose round's net was moved off its definition is
/// refused, naming that face.
#[test]
fn a_reopened_body_s_rounds_are_certified_again_and_a_bad_one_is_named() {
    use solidcore::surface::procedural_blend::convert::{from_record, rebuild, to_record};
    use solidcore::surface::procedural_blend::recertify_faces;
    let mut k = kernel(true);
    let mut b = chamfered_box(&mut k);
    let mut rounds = Vec::new();
    for &f in b.face_list() {
        if let Some(SurfaceKind::ProceduralBlend(pb)) =
            b.face(f).unwrap().surface.as_ref().map(Surface::kind)
        {
            rounds.push((f, to_record(&k, pb).unwrap()));
        }
    }
    assert_eq!(rounds.len(), 2);
    for (f, rec) in &rounds {
        let back = rebuild(&k, rec).unwrap();
        assert!(back.recertify(&k).unwrap() <= rec.definition.bound);
        b.face_mut(*f).unwrap().surface = Some(Surface::from(back));
    }
    recertify_faces(&k, &b).unwrap();
    // the second round's net moved by three bounds at one control point
    let (f, rec) = &rounds[1];
    let mut bad = rec.clone();
    let i = bad.net.points.len() / 2;
    let w = bad.net.weights.as_ref().map_or(1.0, |w| w[i]);
    bad.net.points[i][0] += 3.0 * bad.definition.bound * w;
    assert!(rebuild(&k, &bad).is_err());
    b.face_mut(*f).unwrap().surface = Some(Surface::from(from_record(&k, &bad).unwrap()));
    let e = recertify_faces(&k, &b).unwrap_err();
    assert_eq!(e.entity(), Some(solidcore::EntityRef::Face(*f)), "{e}");
    // a support labelled a kind its NURBS form is not: refused on rebuild, naming the face
    let (f, rec) = &rounds[0];
    let mut bad = rec.clone();
    let solidcore::surface::procedural_blend::record::SupportRecord::Nurbs { kind, .. } =
        &mut bad.definition.supports[0]
    else {
        panic!("the box's round's support is not a NURBS support");
    };
    assert_eq!(
        *kind,
        solidcore::surface::procedural_blend::record::SupportKindRecord::Plane
    );
    *kind = solidcore::surface::procedural_blend::record::SupportKindRecord::Torus;
    let e =
        solidcore::surface::procedural_blend::convert::rebuild_for_face(&k, *f, &bad).unwrap_err();
    assert_eq!(e.entity(), Some(solidcore::EntityRef::Face(*f)), "{e}");
}

// ---- #2388, contract 5: S as the operand downstream ------------------------------------

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use solidcore::math::{Point2, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::topology::FaceId;

/// The body's rounds by definition: each face and its surface.
fn rounds_of(b: &Brep) -> Vec<(FaceId, Surface)> {
    b.face_list()
        .iter()
        .filter_map(|&f| {
            let s = b.face(f).unwrap().surface.clone()?;
            matches!(s.kind(), SurfaceKind::ProceduralBlend(_)).then_some((f, s))
        })
        .collect()
}

/// The body reopened: each round rebuilt from its record (re-certified), as a design read
/// back from SolidCraft's record is. Returns the rebuilt rounds.
///
/// Each rebuilt round is owned by its face, as every face surface is upstream
/// (`SmFace::SetSurface` sets it). The surface keeps the face's identity and its edge uses'
/// UV trims: the record rebuilds the same parameters. A surface written into the face slot
/// without an owner has no `SmSurface::GetFace()`. A fillet whose rail merges onto that face
/// (`MergeCurveOnSurfaces`) then fails with `SM_ERR_NULL_POINTER`, 1006, as upstream does.
/// Since #2364 the smooth-vertex End merges the second round's rail onto the reopened round
/// (#2550). The old WR-674 path never merged a rail onto that face, so it hid the missing
/// owner. `set_surface` would also notify a change of geometry, which drops the UV trims;
/// the fillet would then re-drop them onto S outside its net scope, which is not what a
/// reopened body has to do.
fn reopen(k: &Kernel, b: &mut Brep) -> Vec<(FaceId, Surface)> {
    use solidcore::surface::procedural_blend::convert::{rebuild, to_record};
    let mut out = Vec::new();
    for (f, s) in rounds_of(b) {
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            unreachable!()
        };
        let mut back = Surface::from(rebuild(k, &to_record(k, pb).unwrap()).unwrap());
        let this = b.owner_identity(solidcore::EntityRef::Face(f));
        back.set_owner(Some(this));
        b.face_mut(f).unwrap().surface = Some(back);
        // the surface reads the face's tolerance through `GetFace()`, as `SetSurface` sets it
        b.sync_face_surface_tolerance(f).unwrap();
        let owned = b.face(f).unwrap().surface.as_ref().unwrap();
        assert_eq!(
            owned.owner(),
            Some(this),
            "the reopened round is owned by its face"
        );
        let back = owned.clone();
        out.push((f, back));
    }
    out
}

/// The distance from `p` to S itself: Gauss-Newton on S's own partials (the
/// definition's, outside a net scope), from the foot on the net (a start only), kept in
/// the surface's domain.
fn to_s(k: &Kernel, s: &Surface, p: Point3) -> f64 {
    foot_on_s(k, s, p).0
}

/// [`to_s`] and whether the foot is past the round's ends (held at an end of u, the
/// round's length, where `p` is beyond S rather than on or off it).
fn foot_on_s(k: &Kernel, s: &Surface, p: Point3) -> (f64, bool) {
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let net = match s.kind() {
        SurfaceKind::ProceduralBlend(pb) => Surface::from(pb.net().clone()),
        _ => s.clone(),
    };
    let mut uv =
        surface_closest_point(k, &net, p).map_or_else(|_| dom.evaluate(0.5, 0.5), |c| c.uvs[0]);
    for _ in 0..12 {
        let d = s.evaluate(k, uv, 1, 1, EvaluateFlags::default()).unwrap();
        let (e, su, sv) = (d[0][0] - p, d[1][0], d[0][1]);
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (r, q) = (-su.dot(e), -sv.dot(e));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        uv = dom.clamp_point(Point2::new(
            uv.x + (c * r - b * q) / det,
            uv.y + (a * q - b * r) / det,
        ));
    }
    let at_end = uv.x <= dom.min().x || uv.x >= dom.max().x;
    ((s.evaluate_point(k, uv).unwrap() - p).length(), at_end)
}

/// The edges bounding face `f`.
fn edges_of(b: &Brep, f: FaceId) -> Vec<EdgeId> {
    let mut out = Vec::new();
    for &l in &b.face(f).unwrap().loops {
        out.extend(b.loop_(l).unwrap().edges.iter().map(|le| le.edge));
    }
    out
}

/// Nine points along edge `e`'s curve.
fn edge_points(k: &Kernel, b: &Brep, e: EdgeId) -> Vec<Point3> {
    let edge = b.edge(e).unwrap();
    let c = edge.curve.as_ref().unwrap();
    (0..=8)
        .map(|i| {
            let t = edge.interval.min() + edge.interval.length() * f64::from(i) / 8.0;
            c.evaluate_point(k, t).unwrap()
        })
        .collect()
}

/// Every edge of a face of `b` that is a piece of one of `rounds` (its middle on that
/// round's S) lies on that S within the edge's own tolerance: `(edges checked, the largest
/// distance in units of tolerance)`.
fn edges_on_s(k: &Kernel, b: &Brep, rounds: &[(FaceId, Surface)]) -> (usize, f64) {
    let (mut n, mut worst) = (0, 0.0_f64);
    for (f, s) in rounds_of(b) {
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        let mid = s.evaluate_point(k, d.evaluate(0.5, 0.5)).unwrap();
        let Some((_, round)) = rounds.iter().find(|(_, r)| to_s(k, r, mid) < 1e-9) else {
            continue;
        };
        for e in edges_of(b, f) {
            let tol = b.edge(e).unwrap().tolerance;
            for p in edge_points(k, b, e) {
                worst = worst.max(to_s(k, round, p) / tol);
            }
            n += 1;
        }
    }
    (n, worst)
}

use solidcore::query::api_queries::surface_closest_point;

/// #2388, contract 5: a boolean cutting a reopened round. A cylinder through the bottom
/// round is taken out of the body as reopened and of the body as built: the same volume
/// and the same count of edges on rounds, every such edge on S within its own tolerance.
#[test]
fn a_boolean_cuts_a_reopened_round_on_its_definition() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::primitive::api_primitives::create_cylinder;
    let mut k = kernel(true);
    let built = chamfered_box(&mut k);
    let mut open = built.clone_standing_for_self().unwrap();
    let rounds = reopen(&k, &mut open);
    let mut results = Vec::new();
    for body in [built, open] {
        let tool = create_cylinder(&mut k, Point3::new(20.0, 1.0, -5.0), 1.5, 10.0).unwrap();
        let mut cut = boolean_difference(&mut k, body, tool).unwrap();
        let v = volume_at(&k, &mut cut, SAME_VOLUME_ACCURACY);
        results.push((cut, v));
    }
    let (n0, _) = edges_on_s(&k, &results[0].0, &rounds);
    let (n1, worst) = edges_on_s(&k, &results[1].0, &rounds);
    assert!(
        n1 > 0 && n1 == n0,
        "edges on rounds: {n1} reopened, {n0} built"
    );
    assert!(worst <= 1.0, "an edge {worst} of its tolerance off S");
    assert!(
        (results[0].1 - results[1].1).abs() <= 1e-12 * results[0].1,
        "volumes {} and {}",
        results[0].1,
        results[1].1
    );
}

/// The reopened body offset 0.5 outward, with its rounds: each round's offset face and
/// the round it is the offset of.
fn offset_rounds(k: &mut Kernel) -> (Brep, Vec<(FaceId, Surface)>) {
    let mut b = chamfered_box(k);
    let rounds = reopen(k, &mut b);
    let offset = offset_fixture::offset_full(k, &mut b, 0.5).unwrap();
    let mut out = Vec::new();
    for &f in offset.face_list() {
        let face = offset.face(f).unwrap();
        let s = face.surface.as_ref().unwrap();
        let mid = s
            .evaluate_point(k, face.uv_domain.evaluate(0.5, 0.5))
            .unwrap();
        // the offset of a round: its middle 0.5 off that round's S
        if let Some((_, round)) = rounds
            .iter()
            .find(|(_, r)| (to_s(k, r, mid) - 0.5).abs() < 1e-3)
        {
            out.push((f, round.clone()));
        }
    }
    (offset, out)
}

/// #2388, contract 5: the reopened body offset 0.5 outward: each round's offset face
/// stands 0.5 off its S along its edges (where they lie over the round, not over its
/// extension past the round's ends) and at its internal point, within each edge's and
/// the face's own tolerance. (A shell of this body, open at the top or the bottom, fails
/// as built too, with or without the definitions: a merge's `make_faceuse` or a point
/// classification refuses it, so the offset is the operand here.) The rounds' bound is
/// about 1e-12 here, so this does not tell S from its net: see the next test.
#[test]
fn an_offset_of_a_reopened_round_stands_off_its_definition() {
    use solidcore::query::api_queries::face_internal_point;
    let mut k = kernel(true);
    let (mut offset, walls) = offset_rounds(&mut k);
    assert_eq!(walls.len(), 2, "the rounds' offsets");
    let mut worst = 0.0_f64;
    for (f, round) in &walls {
        for e in edges_of(&offset, *f) {
            let tol = offset.edge(e).unwrap().tolerance;
            for p in edge_points(&k, &offset, e) {
                // (the offset is extended past the round's ends, over the corners)
                let (d, beyond) = foot_on_s(&k, round, p);
                if !beyond {
                    worst = worst.max((d - 0.5).abs() / tol);
                }
            }
        }
        let tol = offset.face(*f).unwrap().tolerance;
        let p = face_internal_point(&k, &mut offset, *f).unwrap();
        worst = worst.max((to_s(&k, round, p) - 0.5).abs() / tol);
    }
    assert!(
        worst <= 1.0,
        "an offset {worst} of its tolerance off 0.5 from S"
    );
}

/// #2388, contract 5: the offset of a round is built on S. Its offset face's surface is an
/// offset surface of the round by its definition (the procedural blend extended exactly
/// past its sides, its across length measured with an honest rounding bound), not of the
/// round's net B (`TurnToNURBS` slices a blend to its net, fork 0136260).
#[test]
fn the_offset_of_a_round_is_the_offset_of_its_definition() {
    let mut k = kernel(true);
    let (offset, walls) = offset_rounds(&mut k);
    assert_eq!(walls.len(), 2, "the rounds' offsets");
    for (f, _) in &walls {
        let s = offset.face(*f).unwrap().surface.as_ref().unwrap();
        let SurfaceKind::OffsetSurface(o) = s.kind() else {
            panic!("not an offset surface: {}", s.type_code());
        };
        assert_eq!(
            o.base_surface().type_code(),
            PROCEDURAL_BLEND_TYPE,
            "the offset's base is not the round by its definition"
        );
    }
}

/// #2388, contract 5, #2451: the plane x = 20 sections each reopened round's face (the
/// face's own section, `SmBrep::CreatePlanarSectionCurves`' per-face step, with no
/// tolerance given). Each section curve lies within the face's tolerance of the round at
/// 65 points along it, and off S and B alike (the net is S to rounding there). With
/// `planar_section_face_tolerance` (WR-723) the section is traced to the face's
/// tolerance; upstream's march runs to its default 1/1000 of the surface's size and left
/// the round by up to 32 times the face's tolerance (5.2e-3 at 1.6e-4).
#[test]
fn a_plane_section_of_a_round_is_within_its_tolerance() {
    use solidcore::topology::brep::planar_section::face_planar_section_curves;
    let mut k = kernel(true);
    k.fixes.planar_section_face_tolerance = true;
    let mut b = chamfered_box(&mut k);
    let rounds = reopen(&k, &mut b);
    let (mut on, mut worst, mut s_vs_b) = (0, 0.0_f64, 0.0_f64);
    for (f, s) in &rounds {
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            unreachable!()
        };
        let net = Surface::from(pb.net().clone());
        let (tol, dom) = {
            let face = b.face(*f).unwrap();
            (face.tolerance, face.uv_domain)
        };
        let section = face_planar_section_curves(
            &k,
            &mut b,
            *f,
            s,
            &dom,
            Point3::new(20.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            None,
            None,
        )
        .unwrap();
        for c in &section.curves_3d {
            let ivl = c.natural_interval();
            for i in 0..=64 {
                let p = c
                    .evaluate_point(&k, ivl.min() + ivl.length() * f64::from(i) / 64.0)
                    .unwrap();
                let (ds, db) = (to_s(&k, s, p), to_s(&k, &net, p));
                on += 1;
                worst = worst.max(ds / tol);
                s_vs_b = s_vs_b.max((ds - db).abs());
            }
        }
    }
    assert!(on > 0, "no section of a round");
    assert!(
        s_vs_b < 1e-10,
        "the section is off S and B differently: {s_vs_b:e}"
    );
    assert!(
        worst <= 1.0,
        "the section leaves the round by {worst} times the face's tolerance"
    );
}

/// #2388, contract 5: distance and measure. A point 1 off the middle of each reopened
/// round along S's normal is 1 from the round (the closest point query's foot on S), and
/// the reopened body's volume is the built one's.
#[test]
fn distance_and_volume_of_a_reopened_round_are_its_definition_s() {
    let mut k = kernel(true);
    let built = chamfered_box(&mut k);
    let mut open = built.clone_standing_for_self().unwrap();
    let rounds = reopen(&k, &mut open);
    for (f, s) in &rounds {
        let tol = open.face(*f).unwrap().tolerance;
        let d = s.natural_uv_domain_with_nurb(&k).unwrap();
        let uv = d.evaluate(0.5, 0.5);
        let e = s.evaluate(&k, uv, 1, 1, EvaluateFlags::default()).unwrap();
        let n = e[1][0].cross(e[0][1]);
        let p = e[0][0] + n * (1.0 / n.length());
        let c = surface_closest_point(&k, s, p).unwrap();
        let foot = s.evaluate_point(&k, c.uvs[0]).unwrap();
        assert!(
            (foot - c.closest.point).length() <= tol,
            "the foot is not S's"
        );
        assert!(
            (c.closest.distance - 1.0).abs() <= tol,
            "distance {}",
            c.closest.distance
        );
    }
    let mut built = built;
    let (v0, v1) = (volume_of(&k, &mut built), volume_of(&k, &mut open));
    assert!((v0 - v1).abs() <= 1e-12 * v0, "volumes {v0} and {v1}");
}

/// A plate with a pipe lying on it tilted 4 degrees down along x (its axis from
/// (0, 20, 16)), its far root edge (a curve: the plate's plane cuts the tilted cylinder
/// in an ellipse) rounded r 2 at the equal area default. The round's section turns along
/// it, so its certified net is a measurable distance off its definition.
fn tilted_pipe(k: &mut Kernel) -> Brep {
    tilted_pipe_rounded(k, 2, 2, GN_DEFAULT_THUMBWEIGHT)
}

/// [`tilted_pipe`] with the root edge rounded r 2 by the cross-section `xsect` of
/// `continuity` and `thumbweight` (`fillet_edges`'s).
fn tilted_pipe_rounded(k: &mut Kernel, xsect: u32, continuity: u32, thumbweight: f64) -> Brep {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::primitive::api_primitives::create_cylinder;
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let phi = 4.0_f64.to_radians();
    let place = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(-phi.sin(), 0.0, -phi.cos()),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(k, &mut pipe, &place, None, false).unwrap();
    let mut b = boolean_union(k, plate, pipe).unwrap();
    // the root edge nearest (30, 29.2, 10)
    let q = Point3::new(30.0, 29.2, 10.0);
    let mut best = (1.0_f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, &b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    let e = best.1.expect("the root edge");
    fillet_edges(k, &mut b, &[e], 2.0, xsect, continuity, thumbweight).unwrap();
    b
}

/// The largest distance between S and its net B over a 41 x 9 grid of the round.
fn s_to_net(k: &Kernel, s: &Surface) -> f64 {
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        panic!("not a blend")
    };
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut gap = 0.0_f64;
    for i in 0..=40 {
        for j in 0..=8 {
            let uv = d.evaluate(f64::from(i) / 40.0, f64::from(j) / 8.0);
            let (p, q) = (
                s.evaluate_point(k, uv).unwrap(),
                pb.net().evaluate_point(uv).unwrap(),
            );
            gap = gap.max((p - q).length());
        }
    }
    gap
}

/// #2388, contract 5, discriminating: the tilted pipe's round, reopened, whose certified
/// net B stands measurably off S (at least 1e3 times the coordinates' rounding, 60 mm x
/// 2.2e-16). A cylinder cut through it leaves edges on S within their own tolerance and
/// measurably off B; a point 1 off S's middle is 1 from the round by the closest point
/// query and not 1 from B; the reopened body's volume is the built one's (the volume
/// integrates S; at its accuracy, 1e-5 of the volume, B's distance does not show).
#[test]
fn a_reopened_round_measurably_off_its_net_is_the_operand_on_s() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::primitive::api_primitives::create_cylinder;
    let rounding = 1e3 * 60.0 * f64::EPSILON;
    let mut k = kernel(true);
    let built = tilted_pipe(&mut k);
    let mut open = built.clone_standing_for_self().unwrap();
    let rounds = reopen(&k, &mut open);
    assert_eq!(rounds.len(), 1, "the root round");
    let (f, s) = &rounds[0];
    let tol = open.face(*f).unwrap().tolerance;
    let gap = s_to_net(&k, s);
    assert!(gap > rounding && gap < tol, "S and B {gap:e} apart");
    let net = match s.kind() {
        SurfaceKind::ProceduralBlend(pb) => Surface::from(pb.net().clone()),
        _ => unreachable!(),
    };

    // distance: 1 off S's middle along its normal
    let d = s.natural_uv_domain_with_nurb(&k).unwrap();
    let mut off_b = 0.0_f64;
    for (a, c) in [(0.3, 0.5), (0.5, 0.5), (0.7, 0.4)] {
        let e = s
            .evaluate(&k, d.evaluate(a, c), 1, 1, EvaluateFlags::default())
            .unwrap();
        let n = e[1][0].cross(e[0][1]);
        let p = e[0][0] + n * (1.0 / n.length());
        let c = surface_closest_point(&k, s, p).unwrap();
        assert!(
            (c.closest.distance - 1.0).abs() <= tol,
            "{}",
            c.closest.distance
        );
        assert!((s.evaluate_point(&k, c.uvs[0]).unwrap() - c.closest.point).length() <= tol);
        off_b = off_b.max((to_s(&k, &net, p) - 1.0).abs());
    }

    // boolean: a cylinder through the round
    let tool = create_cylinder(&mut k, Point3::new(30.0, 30.5, 0.0), 1.5, 30.0).unwrap();
    let operand = solidcore::topology::brep::p3::brep_copy(&mut k, &open).unwrap();
    let cut = boolean_difference(&mut k, operand, tool).unwrap();
    let (n, worst) = edges_on_s(&k, &cut, &rounds);
    assert!(n > 0, "no edge on the round");
    assert!(worst <= 1.0, "an edge {worst} of its tolerance off S");
    let mut edge_off_b = 0.0_f64;
    for (cf, cs) in rounds_of(&cut) {
        let dm = cs.natural_uv_domain_with_nurb(&k).unwrap();
        let mid = cs.evaluate_point(&k, dm.evaluate(0.5, 0.5)).unwrap();
        if to_s(&k, s, mid) > 1e-9 {
            continue;
        }
        for e in edges_of(&cut, cf) {
            for p in edge_points(&k, &cut, e) {
                edge_off_b = edge_off_b.max(to_s(&k, &net, p));
            }
        }
    }
    assert!(
        off_b > rounding && edge_off_b > rounding,
        "B is not measurably off: distance {off_b:e}, edges {edge_off_b:e}"
    );

    // volume
    let mut built = built;
    let (v0, v1) = (
        volume_at(&k, &mut built, SAME_VOLUME_ACCURACY),
        volume_at(&k, &mut open, SAME_VOLUME_ACCURACY),
    );
    assert!((v0 - v1).abs() <= 1e-12 * v0, "volumes {v0} and {v1}");
}

/// The kernel SolidCraft runs: every fix on but `geometric_g2_continuity`, with the round
/// as its definition or as a plain B-spline.
fn solidcraft_kernel(definition: bool) -> Kernel {
    let mut fixes = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *fixes.flag_mut(name).unwrap() = true;
        }
    }
    fixes.fillet_round_as_definition = definition;
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

/// #2388, contract 5 (#2452): a second round whose support is a round. The edge where the
/// bottom round meets the end face x = 0 is rounded r 0.5. Both its ends run on
/// tangentially into the sharp edges x = 0, z = 0 and x = 0, y = 0, which are not picked,
/// so the round ends at its sections through the end vertices (WR-674, trimmed at its
/// creases, WR-706); without those fixes the pinned corner refuses each end
/// (`SmFilletNx1Corner`, "Corner Edge Computation Failure"), which `DoFilleting` returns as
/// its generic status. The round rolls between the plane x = 0 and a surface swept along
/// x, so the ball's centre is at x = 0.5 and its rail on the bottom round is the round's
/// section at x = 0.5, from (0.5, 0, 2) to (0.5, 2, 0): it lies on S within its edge's
/// tolerance, on the body reopened (S the definition) and as built with the rounds as
/// plain B-splines (S the B-spline), and the round adds its face and its two end faces.
///
/// #2510: the round is built to the body's approximation tolerance, far above the reopened
/// round's certified bound (about 1.4e-9), so the reopened round answers it through its
/// net: of about 151 000 evaluations of that face 53 are its definition's, with 10 station
/// solves (before, 146 711 and 59 667: every evaluation took S). Counts, not time: the
/// round is the plain B-spline's, its rail within the bound of the plain one's, the
/// body's volume the plain one's to rounding.
#[test]
fn a_round_on_a_reopened_round_lies_on_it() {
    let mut runs = Vec::new();
    for definition in [true, false] {
        let mut k = solidcraft_kernel(definition);
        let mut b = chamfered_box(&mut k);
        let rounds: Vec<(FaceId, Surface)> = if definition {
            reopen(&k, &mut b)
        } else {
            // the faces whose surface is the G^n section's B-spline (degree 7 in v)
            b.face_list()
                .iter()
                .filter_map(|&f| {
                    let s = b.face(f).unwrap().surface.clone()?;
                    let SurfaceKind::BSpline(bs) = s.kind() else {
                        return None;
                    };
                    (bs.degree(solidcore::nurbs::b_spline_surface::SurfParamType::V) == 7)
                        .then_some((f, s))
                })
                .collect()
        };
        assert_eq!(rounds.len(), 2, "definition {definition}: the rounds");
        // the bottom round (along x near y = z = 0) and its edge in the plane x = 0
        let (f, s) = rounds
            .iter()
            .find(|(_, s)| {
                let d = s.natural_uv_domain_with_nurb(&k).unwrap();
                s.evaluate_point(&k, d.evaluate(0.5, 0.5)).unwrap().z < 5.0
            })
            .unwrap()
            .clone();
        let end = edges_of(&b, f)
            .into_iter()
            .find(|&e| edge_points(&k, &b, e).iter().all(|p| p.x.abs() < 1e-6))
            .unwrap();
        let faces = b.face_list().len();
        k.blend_counts.set(BlendCounts::default());
        fillet_edges(&mut k, &mut b, &[end], 0.5, 1, 1, 0.0)
            .unwrap_or_else(|e| panic!("definition {definition}: {e}"));
        let counts = k.blend_counts.get();
        if let SurfaceKind::ProceduralBlend(pb) = s.kind() {
            // the round's tolerance is above the reopened round's bound: its net answers
            // all but the definition's own work (recorded 53 evaluations, 10 solves of one
            // Newton step, of 150 948), and the session's question (`round_too_small`)
            // whether the round changes the part, which reads the faces' normals across the
            // edge on the reopened round's definition: one evaluation and one solve more
            // (65 evaluations measured with it, 64 before it).
            //
            // The 16 solves are a measured pin, not a derived count. Each is a station solve of
            // one Newton step, on the station memo's miss (4 stations, keyed on exact s and
            // side), traced by caller:
            // - 2: the blend edge use's binormal at s = 20, one per side;
            // - 6: `round_too_small`'s 3 stations, both sides. All 3 lie on the end section
            //   s = 20, but the edge's pcurve, whose control points all have u = 20.0, evaluates
            //   to 20, 20 + 4e-15 and 20 + 7e-15; the third key evicts 20.0, solved again;
            // - 2: the solver's setup, `is_periodic` at s = 60 and 25.714;
            // - 6: the stitch's seam checks (`find_infinite_region`).
            // The 3 solves above the 13 measured before the section's curve ended on the crossing
            // (#2907) are that evaluator drift (#3159). The ceiling becomes a derived count once
            // #3159 is decided.
            assert!(pb.bound() < 1e-8, "the bound {:e}", pb.bound());
            assert!(
                counts.evaluations <= 65
                    && counts.solves <= 16
                    && counts.most_newton_steps <= 1
                    && counts.evaluations * 1000 <= counts.net_evaluations,
                "{counts:?}"
            );
        } else {
            assert_eq!(counts, BlendCounts::default());
        }
        assert_eq!(
            b.face_list().len(),
            faces + 3,
            "definition {definition}: the round and its two end faces"
        );
        // the rail: the edge at x = 0.5 from (0.5, 0, 2) to (0.5, 2, 0)
        let rail: Vec<_> = b
            .edge_list()
            .iter()
            .copied()
            .filter(|&e| {
                let p = edge_points(&k, &b, e);
                p.iter().all(|p| (p.x - 0.5).abs() < 1e-9)
                    && p.iter().any(|p| p.y > 1e-3 && p.z > 1e-3)
            })
            .collect();
        assert_eq!(rail.len(), 1, "definition {definition}: the rail on S");
        let tol = b.edge(rail[0]).unwrap().tolerance;
        let p = edge_points(&k, &b, rail[0]);
        let ends = [p[0], p[8]];
        for q in [Point3::new(0.5, 0.0, 2.0), Point3::new(0.5, 2.0, 0.0)] {
            assert!(
                ends.iter().any(|e| (*e - q).length() <= tol),
                "definition {definition}: the rail does not end at {q:?}: {ends:?}"
            );
        }
        let worst = p.iter().map(|&q| to_s(&k, &s, q)).fold(0.0_f64, f64::max);
        assert!(
            worst <= tol,
            "definition {definition}: the rail {worst:e} off S, its tolerance {tol:e}"
        );
        let bound = match s.kind() {
            SurfaceKind::ProceduralBlend(pb) => pb.bound(),
            _ => 0.0,
        };
        runs.push((b.face_list().len(), p, volume_of(&k, &mut b), bound));
    }
    // #2510: the same round either way: its faces, its rail to the bound, the volume to
    // rounding at part scale
    let (faces, rail, volume, bound) = &runs[0];
    let (plain_faces, plain_rail, plain_volume, _) = &runs[1];
    assert_eq!(faces, plain_faces, "the faces");
    for (a, b) in rail.iter().zip(plain_rail) {
        let d = (*a - *b).length();
        assert!(
            d <= *bound,
            "the rail {d:e} off the plain one's, the bound {bound:e}"
        );
    }
    assert!(
        (volume - plain_volume).abs() <= 1e3 * f64::EPSILON * plain_volume,
        "volumes {volume} and {plain_volume}"
    );
}

/// #2510: a query asking for a tolerance below a reopened round's certified bound goes to
/// S and meets it: inside a scope below the bound every point is the definition's, bit
/// for bit, a definition evaluation each; at the bound the net's, no definition
/// evaluation, within the bound of S.
#[test]
fn a_query_below_the_bound_takes_s_and_one_at_it_the_net() {
    use solidcore::surface::procedural_blend::NetScope;
    let mut k = kernel(true);
    let mut b = chamfered_box(&mut k);
    for (_, s) in reopen(&k, &mut b) {
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            unreachable!()
        };
        let bound = pb.bound();
        let d = s.natural_uv_domain_with_nurb(&k).unwrap();
        for (a, c) in [(0.1, 0.2), (0.5, 0.5), (0.83, 0.9)] {
            let uv = d.evaluate(a, c);
            let on_s = s.evaluate(&k, uv, 1, 1, EvaluateFlags::default()).unwrap();
            k.blend_counts.set(BlendCounts::default());
            let below = NetScope::with_tolerance(&k, 0.5 * bound, || {
                s.evaluate(&k, uv, 1, 1, EvaluateFlags::default()).unwrap()
            });
            let c1 = k.blend_counts.get();
            assert_eq!((c1.evaluations, c1.net_evaluations), (1, 0), "{c1:?}");
            assert_eq!(below, on_s, "below the bound: S");
            let at = NetScope::with_tolerance(&k, bound, || s.evaluate_point(&k, uv).unwrap());
            let c2 = k.blend_counts.get();
            assert_eq!((c2.evaluations, c2.net_evaluations), (1, 1), "{c2:?}");
            assert_eq!(
                at,
                pb.net().evaluate_point(uv).unwrap(),
                "at the bound: the net"
            );
            assert!(
                (at - on_s[0][0]).length() <= bound,
                "the net within its bound of S"
            );
        }
    }
}

// ---- #2487: the offset of a round built from its definition S ----------------------------

/// A ridge's root rounded r 2 by the ball's exact circle (every circular round's section): a
/// procedural blend of order 1 on the plate's plane and the ridge's wall. The 60 x 40 x 10
/// plate carries a ridge extruded from a freeform (cubic Bezier) profile 4 degrees off y.
/// (The tilted pipe's root, a plane against a cylinder, is an exact round class, not a blend
/// by its definition.) Its surface.
fn circle_root(k: &mut Kernel) -> (Brep, Surface) {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::curve::curve::Curve;
    use solidcore::primitive::api_curves::{create_curve, create_line_segment};
    use solidcore::sweep::api_brep::create_linear_sweep;
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let p = |x: f64, z: f64| Point3::new(x, 0.0, z);
    let profile = [
        Curve::from(create_line_segment(p(20.0, 10.0), p(40.0, 10.0)).unwrap()),
        Curve::from(
            create_curve(&[p(40.0, 10.0), p(37.0, 19.0), p(24.0, 16.0), p(20.0, 10.0)]).unwrap(),
        ),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    let phi = 4.0_f64.to_radians();
    let ridge = create_linear_sweep(
        k,
        &refs,
        Vec3::new(phi.sin(), phi.cos(), 0.0),
        40.0 / phi.cos(),
        true,
    )
    .unwrap();
    let mut b = boolean_union(k, plate, ridge).unwrap();
    let q = Point3::new(40.0 + 20.0 * phi.tan(), 20.0, 10.0);
    let mut best = (1.0_f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, &b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    let e = best.1.expect("the root edge");
    fillet_edges(k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
    let s = rounds_of(&b)
        .into_iter()
        .map(|(_, s)| s)
        .next()
        .expect("the root round by its definition");
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        unreachable!()
    };
    assert_eq!((pb.order(), pb.offset(0)), (1, Some(2.0)));
    (b, s)
}

/// S's unit normal at `uv` (its definition's partials).
fn unit_normal_of(k: &Kernel, s: &Surface, uv: Point2) -> (Point3, Vec3) {
    let e = s.evaluate(k, uv, 1, 1, EvaluateFlags::default()).unwrap();
    let n = e[1][0].cross(e[0][1]);
    (e[0][0], n * (1.0 / n.length()))
}

/// #2487: the root round (concave: its normal points at the ball's centre) offset by
/// d = +-0.5 is the procedural blend of radius 2 - d on the plate and the ridge offset by
/// d, on the same leader, seeds and parameters: at every (u, v) of a 41 x 9 grid it is
/// S + d n to rounding at part scale (the ball's circles about the same centres, scaled).
/// Its net is certified, its bound recorded and proved again.
#[test]
fn the_offset_of_a_circle_round_is_the_round_of_r_minus_d_on_the_offset_faces() {
    use solidcore::primitive::api_surface_ops::create_offset_surface;
    let mut k = kernel(true);
    let (_, s) = circle_root(&mut k);
    let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
        unreachable!()
    };
    for d in [0.5, -0.5] {
        let o = create_offset_surface(&k, &s, d).unwrap();
        let SurfaceKind::ProceduralBlend(op) = o.kind() else {
            panic!("d {d}: the offset is a {}", o.type_code());
        };
        assert_eq!(op.order(), 1);
        assert_eq!(op.offset(0), Some(2.0 - d), "d {d}: the ball's radius");
        assert_eq!(op.definition_map(), pb.definition_map());
        assert!(
            op.bound() > 0.0 && op.bound() < 1e-4,
            "d {d}: bound {:e}",
            op.bound()
        );
        let proved = op.recertify(&k).unwrap();
        assert!(proved <= op.bound(), "d {d}");
        // each support is the face's offset by d: at the same (u, v), d along its normal
        for i in 0..2 {
            let (a, b) = (pb.support(i).unwrap(), op.support(i).unwrap());
            let uv = a
                .natural_uv_domain_with_nurb(&k)
                .unwrap()
                .evaluate(0.3, 0.6);
            let (p, n) = unit_normal_of(&k, a, uv);
            let q = b.evaluate_point(&k, uv).unwrap();
            let moved = (q - p).dot(n);
            assert!(
                (moved.abs() - 0.5).abs() <= 1e-12 && (q - p).cross(n).length() <= 1e-12,
                "d {d}: support {i} moved {moved}"
            );
        }
        let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
        let mut worst = 0.0_f64;
        for i in 0..=40 {
            for j in 0..=8 {
                let uv = dom.evaluate(f64::from(i) / 40.0, f64::from(j) / 8.0);
                let (p, n) = unit_normal_of(&k, &s, uv);
                let q = o.evaluate_point(&k, uv).unwrap();
                worst = worst.max((q - (p + n * d)).length());
            }
        }
        assert!(worst <= 1e-9, "d {d}: the offset is {worst:e} off S + d n");
    }
}

/// #2487: the round of radius 2 - d collapses at d >= 2 (its ball has no radius), so the
/// offset there is no surface, as a cylinder's; short of it, it is the round.
#[test]
fn a_circle_round_collapses_at_its_radius() {
    let mut k = kernel(true);
    let (_, s) = circle_root(&mut k);
    for (d, collapses) in [(1.5, false), (2.0, true), (2.5, true), (-2.5, false)] {
        assert_eq!(s.offset_collapses(&k, d, 1e-3).unwrap(), collapses, "d {d}");
        assert_eq!(
            s.create_offset_surface(&k, d, 1e-3).unwrap().is_none(),
            collapses,
            "d {d}"
        );
    }
}

/// #2487: the body offset 0.5 (its convex edges not extended) keeps the round's offset as
/// the round of radius 1.5 by its definition, certified.
#[test]
fn a_body_s_circle_round_offsets_to_its_round_of_r_minus_d() {
    use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
    let mut k = kernel(true);
    let (mut b, _) = circle_root(&mut k);
    let o = offset_brep_simple(
        &mut k,
        &mut b,
        OffsetOptions {
            distance: 0.5,
            extend_convex_edges: false,
            merge_results: true,
        },
    )
    .unwrap();
    let rounds = rounds_of(&o);
    assert_eq!(rounds.len(), 1, "the round's offset by its definition");
    let SurfaceKind::ProceduralBlend(op) = rounds[0].1.kind() else {
        unreachable!()
    };
    assert_eq!(op.offset(0), Some(1.5));
    assert!(op.recertify(&k).unwrap() <= op.bound());
}

/// #2487: with every fix on the chamfered box offsets (it was refused with status 1001:
/// the offset's fold certificate, S1's `certify_regular`, raised the G2 rounds' rational
/// septic nets to degree 112, past its Bernstein forms' cap), to the same faces as with
/// #2388's fix set.
#[test]
fn the_chamfered_box_offsets_with_every_fix_on() {
    let mut faces = Vec::new();
    for all in [false, true] {
        let mut k = kernel(true);
        if all {
            k.fixes = UpstreamFixes::all();
        }
        let mut b = chamfered_box(&mut k);
        let o = offset_fixture::offset_full(&mut k, &mut b, 0.5).unwrap();
        faces.push(o.face_list().len());
    }
    assert_eq!(faces[0], faces[1], "{faces:?}");
}
