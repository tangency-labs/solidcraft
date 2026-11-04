//! tangency-labs/solidcore#2611 (SolidCraft #11744): thickening a half-pipe sheet trimmed
//! along an arc failed with 1001 (`TopologyIntersector::Relate`: a vertex already has a
//! different mate) or left a body that is no closed solid.
//!
//! The sheet is the outer face (radius 25) of a half annulus r 23..25 extruded z -40..40,
//! cut away below y = 1 for z < 0, below an arc of radius `R` (centre (z, y) = (0, 1 + R),
//! from (0, 1) through `sweep` degrees) and past the arc's end in z. Its free boundary has
//! two lines, two circular arcs and two B-spline intersection curves; each line meets its
//! B-spline tangentially at z = 0.
//!
//! Thickening extends and trims each free edge's offset (`TrimExtendedEdge`). Where the
//! next edge is free too, upstream ends the offset where the two edges' UV trim curves
//! cross, and then reads that UV-curve parameter on the lifted 3D curve, whose
//! parameterisation differs: the B-spline's offset ended up to 4.3e-5 (four tolerances)
//! from the arc's at their shared corner, and 6e-6 along the curve at the tangent
//! junction, where the crossing is also ill-conditioned. The merge of the side walls then
//! failed (R14 to R40 inward) or split slivers off the walls (R12, R18 outward).
//! `offset_lamina_corner_at_vertex_image` (WR-736) ends the offset at the vertex's own
//! image `V + d n(V)`.
//!
//! Every result is graded against the closed form: each side wall is ruled along the
//! surface normals, so the solid is the set of radii between 25 and 25 + t over the
//! sheet's (theta, z) region, of volume `(r_out^2 - r_in^2) / 2` times the integral over
//! z of the region's angular width (Simpson's rule on the arc's stretch, exact elsewhere).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use offset_fixture::{api_shell, volume};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p7::copy_faces;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, EntityRef, FaceId, Kernel, SolidError};

/// Relative tolerance on the volume: the integration is asked for 1e-6; the results come
/// within 3.7e-7 of the closed form.
const REL: f64 = 1e-6;

const PIPE_R: f64 = 25.0;

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(curves::create_line_segment(a, b).unwrap())
}

fn extrude(k: &mut Kernel, cs: &[Curve], dir: Vec3, h: f64) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, dir, h, true).unwrap()
}

/// The half annulus r 23..25 (y >= 0) extruded z -40..40.
fn pipe(k: &mut Kernel) -> Brep {
    let mut inner = curves::create_arc(p(0.0, 0.0, -40.0), 23.0, 0.0, 180.0).unwrap();
    let ivl = inner.natural_interval();
    inner.reverse_parameterization(&ivl).unwrap();
    let profile = [
        Curve::from(curves::create_arc(p(0.0, 0.0, -40.0), PIPE_R, 0.0, 180.0).unwrap()),
        line(p(-PIPE_R, 0.0, -40.0), p(-23.0, 0.0, -40.0)),
        Curve::from(inner),
        line(p(23.0, 0.0, -40.0), p(PIPE_R, 0.0, -40.0)),
    ];
    extrude(k, &profile, Vec3::new(0.0, 0.0, 1.0), 80.0)
}

/// The end of the trim arc of radius `r` through `sweep` degrees, in (z, y).
fn arc_end(r: f64, sweep: f64) -> (f64, f64) {
    let a = sweep.to_radians();
    (r * a.sin(), 1.0 + r - r * a.cos())
}

/// The cut: the issue's profile in the plane x = -42 with (u, v) = (z, y), swept 84 along
/// +x: y < 1 for z < 0, below the arc of radius `r` for 0 < z < its end, all past it.
fn cutter(k: &mut Kernel, r: f64, sweep: f64) -> Brep {
    let q = |u: f64, v: f64| p(-42.0, v, u);
    let (ue, ve) = arc_end(r, sweep);
    let mut frame = Axis2Placement::default();
    frame
        .set_canonical(
            q(0.0, 1.0 + r),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
    let mut arc = BSplineCurve::create_circle_segment(
        3,
        &frame,
        r,
        -90.0,
        -90.0 + sweep,
        NurbCircleParam::Quadratic,
    )
    .unwrap();
    let ivl = arc.natural_interval();
    arc.reverse_parameterization(&ivl).unwrap();
    let profile = [
        line(q(-45.0, -2.0), q(45.0, -2.0)),
        line(q(45.0, -2.0), q(45.0, 30.0)),
        line(q(45.0, 30.0), q(ue, 30.0)),
        line(q(ue, 30.0), q(ue, ve)),
        Curve::from(arc),
        line(q(0.0, 1.0), q(-45.0, 1.0)),
        line(q(-45.0, 1.0), q(-45.0, -2.0)),
    ];
    extrude(k, &profile, Vec3::new(1.0, 0.0, 0.0), 84.0)
}

/// The face of `b` on the cylinder of radius 25 about z.
fn outer_face(k: &Kernel, b: &Brep) -> FaceId {
    let found: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let d = face.uv_domain;
            [(0.1, 0.2), (0.5, 0.5), (0.9, 0.7)].iter().all(|&(a, c)| {
                let pt = s.evaluate_point(k, d.evaluate(a, c)).unwrap();
                ((pt.x * pt.x + pt.y * pt.y).sqrt() - PIPE_R).abs() < 1e-6
            })
        })
        .collect();
    assert_eq!(found.len(), 1, "one outer face");
    found[0]
}

/// The sheet: the cut pipe's outer face copied into a B-rep of its own (SolidCraft's
/// `surface.offset` by 0).
fn sheet(k: &mut Kernel, r: f64, sweep: f64) -> Brep {
    let pipe = pipe(k);
    let cut = cutter(k, r, sweep);
    let mut part = boolean_difference(k, pipe, cut).unwrap();
    let face = outer_face(k, &part);
    let mut dest = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut state = ArenaNotifyState::new(&dest);
    state.seed(&part);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f| key_of(EntityRef::Face(f));
    let (made, ok) = copy_faces(
        k,
        &mut part,
        &[face],
        &mut dest,
        false,
        &this_face,
        &mut env,
    )
    .unwrap();
    assert!(ok && made.len() == 1);
    dest
}

/// `SmApiShellBrepFull` thickening the sheet by `t` (an offset solid, extended convex
/// edges, merged results).
fn thicken(k: &mut Kernel, sheet: &mut Brep, t: f64) -> solidcore::Result<Brep> {
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: t,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: false,
    };
    api_shell(k, sheet, options, &[])
}

/// The integral over z of the sheet's angular width (radians).
fn angular_integral(r: f64, sweep: f64) -> f64 {
    let width = |y: f64| PI - 2.0 * (y / PIPE_R).asin();
    let (ze, _) = arc_end(r, sweep);
    let f = |z: f64| width(1.0 + r - (r * r - z * z).sqrt());
    let n = 4000;
    let h = ze / f64::from(n);
    let mut s = f(0.0) + f(ze);
    for i in 1..n {
        s += if i % 2 == 1 { 4.0 } else { 2.0 } * f(h * f64::from(i));
    }
    40.0 * width(1.0) + s * h / 3.0
}

/// The thickened sheet's volume: radii from 25 to 25 + t.
fn closed_form(r: f64, sweep: f64, t: f64) -> f64 {
    let (outer, inner) = if t > 0.0 {
        (PIPE_R + t, PIPE_R)
    } else {
        (PIPE_R, PIPE_R + t)
    };
    (outer * outer - inner * inner) / 2.0 * angular_integral(r, sweep)
}

/// Every opt-in fix (`all`) or the pinned defaults, with
/// `offset_lamina_corner_at_vertex_image` as `fix` says.
fn kernel(all: bool, fix: bool) -> Kernel {
    let mut k = Kernel::default();
    if all {
        k.fixes = UpstreamFixes::all();
    }
    k.fixes.offset_lamina_corner_at_vertex_image = fix;
    k
}

/// Thickens the arc-`r` sheet by `t`; the kernel and the result.
fn run(all: bool, fix: bool, r: f64, sweep: f64, t: f64) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = kernel(all, fix);
    let mut s = sheet(&mut k, r, sweep);
    let out = thicken(&mut k, &mut s, t);
    (k, out)
}

/// With the fix: built, a manifold solid of 8 faces (the sheet, its offset and six side
/// walls), `check_brep` clean at level 2, of the closed-form volume.
fn assert_sound(all: bool, r: f64, sweep: f64, t: f64) {
    let (mut k, out) = run(all, true, r, sweep, t);
    let mut b = out.unwrap_or_else(|e| panic!("R{r} sweep {sweep} t {t}: {e}"));
    assert!(
        brep_is_manifold_solid(&b).unwrap(),
        "R{r} sweep {sweep} t {t}: not a manifold solid"
    );
    assert_eq!(b.face_list().len(), 8, "R{r} sweep {sweep} t {t}: faces");
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "R{r} sweep {sweep} t {t}: {lines:?}");
    let v = volume(&k, &mut b).unwrap();
    let want = closed_form(r, sweep, t);
    assert!(
        (v - want).abs() <= REL * want,
        "R{r} sweep {sweep} t {t}: volume {v}, closed form {want} (rel {:.2e})",
        (v - want).abs() / want
    );
}

fn assert_all_sound(all: bool, radii: &[f64], sweep: f64, thicknesses: &[f64]) {
    for &r in radii {
        for &t in thicknesses {
            assert_sound(all, r, sweep, t);
        }
    }
}

/// Both directions, from a tenth to a fifth of the arc's radius at R25.
const THICKNESSES: [f64; 8] = [0.1, 1.0, 3.0, 5.0, -0.1, -1.0, -3.0, -5.0];

#[test]
fn issue_2611_r20_recipe_thickens_both_ways() {
    // the issue's recipe (R20, 45 degrees, thickness 2); inward it failed with 1001 in
    // the merge's Relate, outward it was no closed solid
    assert_all_sound(true, &[20.0], 45.0, &[2.0, -2.0]);
}

#[test]
fn issue_2611_r14_thickens_both_ways() {
    // inward it failed with 1001 (the merge left a sliver of zero width, which
    // ShrinkGeometry refused)
    assert_all_sound(true, &[14.0], 45.0, &[2.0, -2.0]);
}

#[test]
fn issue_2611_r12_and_r18_close() {
    // outward they were no closed solid: slivers split off the side walls (11 and 10
    // faces); inward R18 failed in Relate
    assert_all_sound(true, &[12.0, 18.0], 45.0, &[2.0, -2.0]);
}

#[test]
fn issue_2611_small_arcs_thicken() {
    assert_all_sound(true, &[5.0, 10.0], 45.0, &THICKNESSES);
}

#[test]
fn issue_2611_middle_arcs_thicken() {
    assert_all_sound(true, &[13.0, 16.0], 45.0, &THICKNESSES);
}

#[test]
fn issue_2611_large_arcs_thicken() {
    assert_all_sound(true, &[25.0, 33.0], 45.0, &THICKNESSES);
}

#[test]
fn issue_2611_largest_arc_thickens() {
    assert_all_sound(true, &[40.0], 45.0, &THICKNESSES);
}

#[test]
fn issue_2611_other_arc_sweeps_thicken() {
    // the issue's 10, 60 and 80 degree arcs (R20)
    for sweep in [10.0, 60.0, 80.0] {
        assert_all_sound(true, &[20.0], sweep, &[2.0, -2.0]);
    }
}

#[test]
fn issue_2611_fix_alone_on_the_pinned_defaults() {
    // the fix needs no other opt-in: on the pinned defaults the same sheets close
    assert_all_sound(false, &[14.0, 20.0, 30.0], 45.0, &[2.0, -2.0]);
}

#[test]
fn issue_2611_without_the_fix_inward_r20_fails_in_relate() {
    // upstream behaviour (flag off): the B-spline's offset corner lands 4.3e-5 from the
    // arc's, and the merge's Relate finds a vertex already mated to another
    let (_, out) = run(true, false, 20.0, 45.0, -2.0);
    match out {
        Err(SolidError::Upstream {
            code, operation, ..
        }) => {
            assert_eq!(code, 1001);
            assert!(operation.ends_with("::relate"), "{operation}");
        }
        other => panic!("expected 1001 from Relate, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn issue_2611_without_the_fix_outward_r12_is_no_closed_solid() {
    // upstream behaviour (flag off): slivers split off the side walls at the corners
    let (_, out) = run(true, false, 12.0, 45.0, 2.0);
    let b = out.unwrap();
    assert!(!brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 11);
}

/// Writes the R20 and R12 sheets (`fixtures/offset.offset_executive_p2/data/`) for the
/// `issue2611_*` cases of `offset.offset_executive_p2` (data flows Rust to C++ only).
#[test]
#[ignore = "generator: writes the #2611 fixture files"]
fn write_issue_2611_operands() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::geom::object::ObjectRef;
    use solidcore::topology::api_files::write_brep_to_file;
    use solidcore::topology::brep_data_write::FromBrepEnv;
    use solidcore::topology::topology::NotifyRecords;
    use solidcore::topology::{EdgeId, EdgeuseId};
    for r in [20.0_f64, 12.0] {
        let mut k = Kernel::default();
        let b = sheet(&mut k, r, 45.0);
        let store = AttributeStore::new();
        let records = NotifyRecords::new();
        let fresh = Brep::new();
        let key_of = |e| -> ObjectRef { fresh.owner_identity(e) };
        let env = FromBrepEnv {
            attributes: &store,
            key_of: &key_of,
            records: &records,
            curve_key: &|_: EdgeId| ObjectKey(0),
            uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
            surface_key: &|_: FaceId| ObjectKey(0),
        };
        let bytes = write_brep_to_file(&k, Some(&b), &env, true).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join(format!(
            "fixtures/offset.offset_executive_p2/data/issue2611_half_pipe_arc_r{r}.smb"
        ));
        std::fs::write(&path, &bytes).unwrap();
    }
}
