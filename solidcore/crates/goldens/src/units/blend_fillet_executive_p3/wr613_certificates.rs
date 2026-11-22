//! WR-613 (#18640): a fillet corner patch between a face of the body and a round may take
//! the round's twist at that corner, so the patch meets the face exactly. The kernel
//! takes such a patch only where every side with a surface is proved, piece by piece,
//! within the largest proved lower bound of main's patch over its sides
//! (`face_sided_patch` in `nurbs::b_spline_surface_n_lib::p2`, by
//! [`seam_angle_within`]); otherwise it keeps main's patch bit for bit. Each row is
//! checked here, on the finished body, against main's figures in [`ROWS`] (main's
//! library, measured by this module's own measures: [`record_main_figures`]):
//! - `check_brep` Level 2 reports nothing and the body is one closed manifold solid;
//! - the corner-patch sides on plane faces number [`Row::face_sides`], and the corner
//!   patches' seam edges are main's;
//! - a corner patch whose seams all read main's readings bit for bit is main's patch
//!   (EQUAL); a row with every patch so is a kept row ([`Row::moved`] false), and its
//!   volume and next-row distance are main's bit for bit too;
//! - any other corner patch is a face-sided patch: each of its face sides holds the
//!   tangency certificate of [`patch_sides`] (its next control row in the plane to
//!   [`IN_PLANE`], the net on one side of it), and every one of its seams is proved
//!   ([`edge_seam_angle_enclosure`]'s upper bound) within the largest proved lower bound
//!   of main's patch over the same seams ([`Row::main_patches`]): its largest normal
//!   angle along its seams is no larger than main's patch's (PROVEN);
//! - the row's volume lies in the body's certified volume enclosure ([`enclose_volume`]):
//!   a consistency check of the mass integrator on the new body.
//!
//! The readings ([`seam_angle`]) are printed beside each proof, labelled CLOSER, EQUAL or
//! FARTHER against main's reading of the same seam; they are measurements, not the proof.
//!
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
//! [`seam_angle_within`]: solidcore::query::seam_bound::seam_angle_within
//! [`edge_seam_angle_enclosure`]: solidcore::query::seam_bound::edge_seam_angle_enclosure

use super::{apply, build, RealSolid};
use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::math::Vec3;
use solidcore::query::continuity::{
    edge_seam_normal_angle, seam_angle_farther, seam_angle_rounding, SEAM_DOT_ROUNDING,
};
use solidcore::query::seam_bound::{curve_b_spline, edge_seam_angle_enclosure, seam_angle_lower_bound};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology_traverser::face_edges;
use solidcore::topology::{Brep, EdgeId, FaceId};

/// A row with corner-patch sides on plane faces, and main's figures for it (main's
/// library, measured by this module's own [`patch_sides`], [`seam_angle`] and
/// [`seam_lower`]).
struct Row {
    /// The row's name.
    name: &'static str,
    /// Whether a face-sided patch was taken at some corner (else main's patches are kept).
    moved: bool,
    /// Main's volume (the golden's on main).
    main_volume: f64,
    /// The corner-patch sides on plane faces: the same on main.
    face_sides: usize,
    /// Main's largest next-row distance from those planes.
    main_next_row_off: f64,
    /// Main's largest normal angle on each seam of those patches: (the edge's index in
    /// the body's edge list, radians).
    main_seams: &'static [(usize, f64)],
    /// Each of those patches on main: its seams (edge indices, ascending) and the largest
    /// proved lower bound of the normal angle over them ([`seam_lower`]).
    main_patches: &'static [(&'static [usize], f64)],
}

/// How far a control point may lie off a plane and count as in it: the rounding of
/// the distance `(p - o) . n` as this module reads it and as the kernel placed the
/// point. With coordinates within 30 (asserted in [`patch_sides`]), `|p - o| <= 128`;
/// the difference rounds by `u` a coordinate, the dot product by `γ_3`, and `n`'s
/// length is within `2u` of 1: at most `6u x 128 = 768u` a reading. The kernel put the
/// point in the plane from a boundary control point and a normal of its own, each read
/// to the same order. `8192u` (`9.1e-13`) bounds both readings with a factor 5 to spare.
const IN_PLANE: f64 = 8192.0 * (f64::EPSILON / 2.0);

#[test]
fn angle_rounding_bounds_acos() {
    // the floor is the half-order loss of acos next to 1
    let c = 1.0 - SEAM_DOT_ROUNDING;
    assert!(c.acos() <= seam_angle_rounding(0.0));
    // away from 0 it is the first-order bound, and smaller
    assert!(seam_angle_rounding(6.8e-4) < 5e-12);
    assert!(seam_angle_rounding(6.8e-4) >= SEAM_DOT_ROUNDING / (6.8e-4_f64).sin());
}

/// The row's input, read as the goldens gate reads it (upstream file paths resolved).
fn load(name: &str) -> RealSolid {
    let case = crate::case::regression_case(&super::super::Runner, name);
    let inputs: super::super::Inputs = case.parse().expect("inputs");
    inputs.real_solid.expect("a real_solid case")
}

/// The row's body: its input rounded as the goldens gate rounds it.
fn body(k: &mut Kernel, input: &RealSolid, name: &str) -> Brep {
    let mut body = build(k, input).expect("fixture").expect("a body");
    let edges = p1::edges(&body, None).expect("edges");
    let selected: Vec<_> = if input.all_edges {
        edges
    } else {
        input
            .edge_indices()
            .expect("edge indices")
            .iter()
            .map(|&i| *edges.get(i).expect("selected edge"))
            .collect()
    };
    apply(k, &mut body, &selected, input).unwrap_or_else(|e| panic!("{name}: the round failed: {e:?}"));
    body
}

/// The corner-patch sides on plane faces of `b`: each side of a non-rational B-spline
/// face whose boundary row of control points lies on a plane face the patch shares an
/// edge with (to `1e-9`; the patch's side is then on the plane, as a B-spline curve
/// lies in the hull of its control points).
struct PatchSides {
    /// How many such sides.
    count: usize,
    /// The largest distance of a next row (the row beside the boundary row) from its
    /// side's plane.
    next_row: f64,
    /// Whether every such side holds the certificate: its next row lies in the plane
    /// too (to [`IN_PLANE`]), so the cross-boundary derivative along the side, a
    /// combination of the two rows' differences, lies in the plane and the patch is
    /// tangent to it all along the side; and the whole net lies on one side of the
    /// plane, so the patch, in the convex hull of its net, does not cross it.
    certified: bool,
    /// The patches with such a side, each with whether all its such sides hold the
    /// certificate.
    patches: Vec<(FaceId, bool)>,
}

/// [`PatchSides`] of `b`.
fn patch_sides(b: &Brep) -> PatchSides {
    let faces = p1::faces(b, None).expect("faces");
    let mut planes = Vec::new();
    for &f in &faces {
        let s = solidcore::topology::face::surface(b, f)
            .expect("surface")
            .expect("a surface");
        if let SurfaceKind::Plane(p) = s.kind() {
            let (o, n, _) = p.is_planar();
            assert!(o.x.abs() <= 30.0 && o.y.abs() <= 30.0 && o.z.abs() <= 30.0);
            planes.push((f, o, n));
        }
    }
    let mut out = PatchSides {
        count: 0,
        next_row: 0.0,
        certified: true,
        patches: Vec::new(),
    };
    for &f in &faces {
        let s = solidcore::topology::face::surface(b, f)
            .expect("surface")
            .expect("a surface");
        let SurfaceKind::BSpline(patch) = s.kind() else {
            continue;
        };
        if patch.is_rational() {
            continue;
        }
        let net = patch.control_point_net().expect("net");
        // the coordinates [`IN_PLANE`] is derived for
        assert!(
            net.points
                .iter()
                .all(|p| p.x.abs() <= 30.0 && p.y.abs() <= 30.0 && p.z.abs() <= 30.0),
            "a corner patch net beyond 30 of the origin"
        );
        let (nu, nv) = (net.u_count, net.v_count);
        let at = |i: usize, j: usize| net.points[i * nv + j];
        // each side: its boundary row and the row next to it
        let rows: [(Vec<_>, Vec<_>); 4] = [
            ((0..nv).map(|j| at(0, j)).collect(), (0..nv).map(|j| at(1, j)).collect()),
            (
                (0..nv).map(|j| at(nu - 1, j)).collect(),
                (0..nv).map(|j| at(nu - 2, j)).collect(),
            ),
            ((0..nu).map(|i| at(i, 0)).collect(), (0..nu).map(|i| at(i, 1)).collect()),
            (
                (0..nu).map(|i| at(i, nv - 1)).collect(),
                (0..nu).map(|i| at(i, nv - 2)).collect(),
            ),
        ];
        let patch_edges = face_edges(b, f).expect("patch edges");
        for &(plane, o, n) in &planes {
            // only the plane face the patch meets across an edge: a side along a
            // body edge also lies in the other face at that edge, which the patch
            // does not touch tangentially
            let plane_edges = face_edges(b, plane).expect("plane edges");
            if !patch_edges.iter().any(|e| plane_edges.contains(e)) {
                continue;
            }
            let d = |p: Vec3| (p - o).dot(n);
            for (boundary, next) in &rows {
                let on = boundary.iter().all(|&p| d(p).abs() <= 1e-9);
                let span = boundary[0].distance(boundary[boundary.len() - 1]);
                if !on || span <= 1e-9 {
                    continue;
                }
                let off = next.iter().map(|&p| d(p).abs()).fold(0.0, f64::max);
                let lo = net.points.iter().map(|&p| d(p)).fold(f64::INFINITY, f64::min);
                let hi = net
                    .points
                    .iter()
                    .map(|&p| d(p))
                    .fold(f64::NEG_INFINITY, f64::max);
                // NaN fails both comparisons
                let holds = off <= IN_PLANE && (lo >= -IN_PLANE || hi <= IN_PLANE);
                out.certified &= holds;
                out.count += 1;
                out.next_row = out.next_row.max(off);
                match out.patches.iter_mut().find(|p| p.0 == f) {
                    Some(p) => p.1 &= holds,
                    None => out.patches.push((f, holds)),
                }
            }
        }
    }
    out
}

/// The largest angle between the normals of the two faces at edge `e`, either way
/// round (`min(θ, π - θ)`): the kernel's seam reading ([`edge_seam_normal_angle`], 257
/// even stations along the edge and 129 over each end's 1/32, where a corner patch's
/// kink sits), the very function the kernel's WR-613 guard compares patches with.
fn seam_angle(k: &Kernel, b: &Brep, e: EdgeId) -> f64 {
    edge_seam_normal_angle(k, b, e)
        .expect("continuity")
        .unwrap_or_else(|| panic!("edge {e:?}: too few stations measured, or an angle not finite"))
}

/// The seams of `patches`: each edge's index in `b`'s edge list, with its angle.
fn seams(k: &Kernel, b: &Brep, patches: &[FaceId]) -> Vec<(usize, f64)> {
    let edges = p1::edges(b, None).expect("edges");
    let mut out: Vec<(usize, f64)> = Vec::new();
    for &f in patches {
        for e in face_edges(b, f).expect("patch edges") {
            let index = edges.iter().position(|&x| x == e).expect("a body edge");
            if out.iter().all(|s| s.0 != index) {
                out.push((index, seam_angle(k, b, e)));
            }
        }
    }
    out.sort_by_key(|s| s.0);
    out
}

/// The seams of one patch: each edge's index in `b`'s edge list, ascending.
fn patch_seam_edges(b: &Brep, f: FaceId) -> Vec<usize> {
    let edges = p1::edges(b, None).expect("edges");
    let mut out: Vec<usize> = face_edges(b, f)
        .expect("patch edges")
        .iter()
        .map(|e| edges.iter().position(|x| x == e).expect("a body edge"))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// A proved lower bound of the largest normal angle at seam `e` of `b`, either way round
/// ([`seam_angle_lower_bound`] along the edge's curve between its two faces' surfaces:
/// the largest of its stations' proved lower bounds), `0` where no station's is proved
/// (no bound above `0`).
fn seam_lower(k: &Kernel, b: &Brep, e: EdgeId) -> f64 {
    let mut fs = solidcore::topology::edge::faces(b, e, None).expect("edge faces");
    fs.sort();
    fs.dedup();
    let &[fa, fb] = fs.as_slice() else {
        panic!("edge {e:?}: not a seam of two faces");
    };
    let surface = |f: FaceId| b.face(f).expect("face").surface.as_ref().expect("a surface");
    let edge = b.edge(e).expect("edge");
    let curve = edge.curve.as_ref().expect("an edge curve");
    let bs = curve_b_spline(curve).expect("a B-spline edge curve");
    seam_angle_lower_bound(k, bs, edge.interval, [surface(fa), surface(fb)])
        .expect("lower bound")
        .unwrap_or(0.0)
}

/// Each patch of `patches` in `b`: its seams ([`patch_seam_edges`]) and the largest
/// [`seam_lower`] over them, in the order of the seams' first index.
fn patch_lowers(k: &Kernel, b: &Brep, patches: &[FaceId]) -> Vec<(Vec<usize>, f64)> {
    let edges = p1::edges(b, None).expect("edges");
    let mut out: Vec<(Vec<usize>, f64)> = patches
        .iter()
        .map(|&f| {
            let seams = patch_seam_edges(b, f);
            let lower = seams
                .iter()
                .map(|&i| seam_lower(k, b, edges[i]))
                .fold(0.0, f64::max);
            (seams, lower)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The row named `name`, checked against main's figures in [`ROWS`].
fn check(name: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.name == name).collect();
    assert_eq!(picked.len(), 1, "{name}: one row of that name");
    let row = picked[0];
    let input = load(name);
    let mut k = Kernel::default();
    let mut body = body(&mut k, &input, name);
    let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
        .expect("check")
        .lines();
    assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
    assert!(
        is_manifold_solid(&body).expect("manifold check"),
        "{name}: not one closed manifold solid"
    );
    let golden = super::rust(&input).unwrap_or_else(|e| panic!("{name}: harness: {e:?}"));
    assert_eq!(golden.ints["mass_status"], [0], "{name}: mass status");
    let volume = *golden.values["mass_properties"].get(1).expect("volume");

    let sides = patch_sides(&body);
    assert_eq!(sides.count, row.face_sides, "{name}: corner-patch sides on plane faces");
    let ids: Vec<FaceId> = sides.patches.iter().map(|p| p.0).collect();
    let measured = seams(&k, &body, &ids);
    let edges: Vec<usize> = measured.iter().map(|s| s.0).collect();
    let main_edges: Vec<usize> = row.main_seams.iter().map(|s| s.0).collect();
    assert_eq!(edges, main_edges, "{name}: the corner patches' seams");
    let reading = |i: usize| measured.iter().find(|s| s.0 == i).map(|s| s.1);
    let main_reading = |i: usize| row.main_seams.iter().find(|s| s.0 == i).map(|s| s.1);
    for (&(edge, angle), &(_, main)) in measured.iter().zip(row.main_seams) {
        let label = if angle.to_bits() == main.to_bits() {
            "bit-equal"
        } else if seam_angle_farther(angle, main) {
            "FARTHER"
        } else if angle < main - (seam_angle_rounding(angle) + seam_angle_rounding(main)) {
            "CLOSER"
        } else {
            "EQUAL"
        };
        eprintln!("{name}: edge {edge}: reads {angle:e} rad (main {main:e}): {label}");
    }

    // each patch: main's (every seam reads main's, bit for bit) or proved
    let all_edges = p1::edges(&body, None).expect("edges");
    let mut patches: Vec<(FaceId, bool, Vec<usize>)> = sides
        .patches
        .iter()
        .map(|&(f, certified)| (f, certified, patch_seam_edges(&body, f)))
        .collect();
    patches.sort_by(|a, b| a.2.cmp(&b.2));
    let main_patches: Vec<&[usize]> = row.main_patches.iter().map(|p| p.0).collect();
    let ours: Vec<&[usize]> = patches.iter().map(|p| p.2.as_slice()).collect();
    assert_eq!(ours, main_patches, "{name}: the corner patches' seams, by patch");
    let mut moved = 0;
    for ((f, certified, seams), &(_, main_lower)) in patches.iter().zip(row.main_patches) {
        let kept = seams.iter().all(|&i| {
            reading(i).map(f64::to_bits) == main_reading(i).map(f64::to_bits)
        });
        if kept {
            eprintln!("{name}: patch {f:?} (seams {seams:?}): main's patch kept, readings bit-equal");
            continue;
        }
        moved += 1;
        assert!(*certified, "{name}: patch {f:?}: a face side misses its certificate");
        assert!(main_lower > 0.0, "{name}: patch {f:?}: main's patch has no lower bound");
        let mut worst = 0.0_f64;
        for &i in seams {
            let e = all_edges[i];
            let enc = edge_seam_angle_enclosure(&k, &body, e, main_lower)
                .expect("enclosure")
                .unwrap_or_else(|| panic!("{name}: edge {i}: the angle is not enclosed"));
            assert!(
                enc.upper <= main_lower,
                "{name}: edge {i}: proved at most {:e}, above main's patch's lower bound {main_lower:e}",
                enc.upper
            );
            worst = worst.max(enc.upper);
        }
        eprintln!(
            "{name}: patch {f:?} (seams {seams:?}): face-sided, face sides certified, every seam \
             proved at most {worst:e} <= main's patch's lower bound {main_lower:e}: PROVEN"
        );
    }
    assert_eq!(moved > 0, row.moved, "{name}: {moved} patches moved");

    if !row.moved {
        // main's patches kept: main's figures, bit for bit
        assert_eq!(volume.to_bits(), row.main_volume.to_bits(), "{name}: volume {volume}");
        assert_eq!(
            sides.next_row.to_bits(),
            row.main_next_row_off.to_bits(),
            "{name}: next row {:e}",
            sides.next_row
        );
        eprintln!("{name}: main's patches kept, every figure equal");
        return;
    }

    // the integrator's volume of the new body lies in its certified enclosure
    let e = enclose_volume(&k, &mut body, 1e-7 * volume).expect("enclosure");
    eprintln!(
        "{name}: volume in [{}, {}] (width {:e}, {:?}), row {volume}, main {}",
        e.lo,
        e.hi,
        e.width(),
        e.stop,
        row.main_volume
    );
    assert!(
        e.lo <= volume && volume <= e.hi,
        "{name}: row volume {volume} not in [{}, {}]",
        e.lo,
        e.hi
    );
}

/// Prints [`ROWS`]'s figures as measured on the library this runs on: run on main's
/// library (with this module and `query::{continuity, curve_surface_gap, seam_bound}`
/// copied in; they only measure) to record main's figures. `moved` is printed false.
#[test]
#[ignore = "records main's figures; run on main's library"]
fn record_main_figures() {
    for row in ROWS {
        let name = row.name;
        let input = load(name);
        let mut k = Kernel::default();
        let body = body(&mut k, &input, name);
        let golden = super::rust(&input).unwrap_or_else(|e| panic!("{name}: harness: {e:?}"));
        let volume = *golden.values["mass_properties"].get(1).expect("volume");
        let sides = patch_sides(&body);
        let ids: Vec<FaceId> = sides.patches.iter().map(|p| p.0).collect();
        let seams = seams(&k, &body, &ids);
        let lowers = patch_lowers(&k, &body, &ids);
        let seams: Vec<String> = seams.iter().map(|(i, a)| format!("({i}, {a:e})")).collect();
        let lowers: Vec<String> = lowers
            .iter()
            .map(|(s, l)| format!("(&{s:?}, {l:e})"))
            .collect();
        println!(
            "    Row {{ name: \"{name}\", moved: false, main_volume: {volume:?}, face_sides: {}, \
             main_next_row_off: {:e}, main_seams: &[{}], main_patches: &[{}] }},",
            sides.count,
            sides.next_row,
            seams.join(", "),
            lowers.join(", ")
        );
    }
}

/// The 24 rows with corner-patch sides on plane faces, each with main's figures
/// (recorded from main's library at commit `30f970e49e` by [`record_main_figures`])
/// and whether a face-sided patch is taken.
const ROWS: &[Row] = &[
    Row { name: "corner3x2-union-inset-side-step-edges1-14-r1", moved: false, main_volume: 2600.6420851687185, face_sides: 1, main_next_row_off: 3.832959412086723e-5, main_seams: &[(30, 5.923077239585752e-3), (31, 4.944232823646056e-3), (32, 6.23440552138508e-3), (37, 1.3344925395341082e-2)], main_patches: &[(&[30, 31, 32, 37], 6.234405338012741e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges1-18-r1", moved: false, main_volume: 2599.998279430282, face_sides: 1, main_next_row_off: 3.8329594122643584e-5, main_seams: &[(30, 5.922816023582481e-3), (31, 4.944232825801736e-3), (32, 6.234405521278232e-3), (37, 5.1714294764415926e-5)], main_patches: &[(&[30, 31, 32, 37], 6.234405337636447e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges12-13-r1", moved: false, main_volume: 2600.6420849383753, face_sides: 1, main_next_row_off: 3.8329594121755406e-5, main_seams: &[(30, 5.923341031882552e-3), (31, 4.94423282568946e-3), (32, 6.234405520797412e-3), (37, 3.5114351597007764e-5)], main_patches: &[(&[30, 31, 32, 37], 6.234405453363686e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges12-17-r1", moved: false, main_volume: 2599.998279429344, face_sides: 1, main_next_row_off: 3.8329594122643584e-5, main_seams: &[(30, 5.923341213055276e-3), (31, 4.944232825846645e-3), (32, 6.234405521082342e-3), (37, 1.1304786152093377e-4)], main_patches: &[(&[30, 31, 32, 37], 6.234405453949019e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges13-19-r1", moved: false, main_volume: 2600.6420848956946, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 5.92327525610822e-3), (30, 4.944232826610115e-3), (31, 6.234405521135767e-3), (37, 3.105872445006255e-5)], main_patches: &[(&[29, 30, 31, 37], 6.234405400107861e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges14-27-r1", moved: false, main_volume: 2600.642084939552, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(28, 5.923276997342284e-3), (29, 4.94423282613856e-3), (30, 6.234405521723435e-3), (37, 1.4604528939570931e-3)], main_patches: &[(&[28, 29, 30, 37], 6.234405396329352e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges15-19-r1", moved: false, main_volume: 2601.0712885699536, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(30, 5.923209734424276e-3), (31, 4.9442328278675955e-3), (32, 6.234405521527545e-3), (37, 5.496506621916365e-3)], main_patches: &[(&[30, 31, 32, 37], 6.234405404016594e-3)] },
    Row { name: "corner3x2-union-inset-side-step-edges15-27-r1", moved: false, main_volume: 2601.0712886131096, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 5.92359981746963e-3), (30, 4.944232824723896e-3), (31, 6.234405521331656e-3), (37, 5.3033234788566415e-5)], main_patches: &[(&[29, 30, 31, 37], 6.234405399372761e-3)] },
    Row { name: "corner3x2-union-overhang-corner-edges6-12-r1", moved: false, main_volume: 2598.710668595942, face_sides: 1, main_next_row_off: 3.8329594122643584e-5, main_seams: &[(28, 5.923242069642571e-3), (29, 4.944232826250836e-3), (30, 6.234405521224807e-3), (36, 3.113719844376749e-5)], main_patches: &[(&[28, 29, 30, 36], 6.2344054050131464e-3)] },
    Row { name: "corner3x2-union-overhang-corner-edges6-13-r1", moved: false, main_volume: 2599.5690759417034, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(28, 5.9232393258770236e-3), (29, 4.94423282537509e-3), (30, 6.234405521830283e-3), (36, 3.1215275986131e-5)], main_patches: &[(&[28, 29, 30, 36], 6.234405401321052e-3)] },
    Row { name: "corner3x2-union-overhang-front-edges15-22-r1", moved: false, main_volume: 2599.998279385677, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(28, 5.923210169747027e-3), (29, 4.9442328272613105e-3), (30, 6.234405521260424e-3), (35, 5.496515018633687e-3)], main_patches: &[(&[28, 29, 30, 35], 6.234405399689814e-3)] },
    Row { name: "corner3x2-union-overhang-front-edges15-24-r1", moved: false, main_volume: 2599.99827942793, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(28, 5.9230795616648105e-3), (29, 4.94423282568946e-3), (30, 6.2344055211535744e-3), (35, 1.0800134906699555e-2)], main_patches: &[(&[28, 29, 30, 35], 6.234405396846787e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.1", moved: true, main_volume: 950.6479346142942, face_sides: 4, main_next_row_off: 3.7687186704715714e-11, main_seams: &[(26, 2.1476597587614735e-3), (27, 0e0), (28, 4.307331468154267e-3), (29, 4.308603566563169e-3), (30, 0e0), (31, 2.1490285982051775e-3), (32, 2.1476595554466467e-3), (33, 0e0), (34, 4.308606168864467e-3), (35, 4.307331512539287e-3), (36, 0e0), (37, 2.14766389173986e-3), (70, 6.10261808574068e-3), (72, 6.102617056435018e-3), (73, 6.102618668961659e-3), (75, 6.102617784087809e-3)], main_patches: &[(&[26, 27, 28, 70], 6.1026180673475604e-3), (&[29, 30, 31, 72], 6.102617036055068e-3), (&[32, 33, 34, 73], 6.1026186503080025e-3), (&[35, 36, 37, 75], 6.102617764060949e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.25", moved: true, main_volume: 949.2763535526122, face_sides: 4, main_next_row_off: 4.3016790129968285e-10, main_seams: &[(26, 1.065590352696303e-3), (27, 1.4901161193847656e-8), (28, 1.7413585711547235e-3), (29, 1.8175831203211463e-3), (30, 1.4901161193847656e-8), (31, 1.0655903441528411e-3), (32, 1.0655903540507544e-3), (33, 1.4901161193847656e-8), (34, 1.8175831143350696e-3), (35, 1.7413585795067821e-3), (36, 1.4901161193847656e-8), (37, 1.0655903455072924e-3), (70, 2.1250223989407236e-3), (72, 2.125022396380705e-3), (73, 2.125022400351346e-3), (75, 2.125022401239516e-3)], main_patches: &[(&[26, 27, 28, 70], 2.1250223844633577e-3), (&[29, 30, 31, 72], 2.125022382858125e-3), (&[32, 33, 34, 73], 2.125022386072079e-3), (&[35, 36, 37, 75], 2.12502238715012e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r0.5", moved: true, main_volume: 944.4771092314386, face_sides: 4, main_next_row_off: 8.602851764294428e-10, main_seams: &[(24, 2.927077601423078e-3), (25, 1.4901161193847656e-8), (26, 3.76907175142291e-3), (27, 3.769071754987111e-3), (28, 1.4901161193847656e-8), (29, 2.927077588261557e-3), (30, 2.927077600436912e-3), (31, 1.4901161193847656e-8), (32, 3.7690717561359036e-3), (33, 3.7690717516585598e-3), (34, 1.4901161193847656e-8), (35, 2.9270775902338884e-3), (69, 4.947288444525646e-3), (71, 4.947288441428769e-3), (73, 4.947288439453949e-3), (75, 4.947288441810268e-3)], main_patches: &[(&[24, 25, 26, 69], 4.9472884332286745e-3), (&[27, 28, 29, 71], 4.947288430642731e-3), (&[30, 31, 32, 73], 4.947288428519583e-3), (&[33, 34, 35, 75], 4.94728843116927e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-r1.0", moved: true, main_volume: 925.9490134066059, face_sides: 4, main_next_row_off: 1.070619148890728e-10, main_seams: &[(24, 7.0176816800739525e-3), (25, 0e0), (26, 6.777931877701719e-3), (27, 6.7779318764404515e-3), (28, 0e0), (29, 7.017681677052238e-3), (30, 7.017681683079847e-3), (31, 0e0), (32, 6.777931874212759e-3), (33, 6.777931872984252e-3), (34, 0e0), (35, 7.017681676055546e-3), (69, 8.385535504448382e-3), (71, 8.385535508618949e-3), (73, 8.385535503071433e-3), (75, 8.385535509082345e-3)], main_patches: &[(&[24, 25, 26, 69], 8.385535488308981e-3), (&[27, 28, 29, 71], 8.385535492709378e-3), (&[30, 31, 32, 73], 8.385535487342435e-3), (&[33, 34, 35, 75], 8.38553549287527e-3)] },
    Row { name: "curve-sih-box_diff_cyl-all-edges-step2", moved: true, main_volume: 950.6479346142942, face_sides: 4, main_next_row_off: 3.7687186704715714e-11, main_seams: &[(26, 2.1476597587614735e-3), (27, 0e0), (28, 4.307331468154267e-3), (29, 4.308603566563169e-3), (30, 0e0), (31, 2.1490285982051775e-3), (32, 2.1476595554466467e-3), (33, 0e0), (34, 4.308606168864467e-3), (35, 4.307331512539287e-3), (36, 0e0), (37, 2.14766389173986e-3), (70, 6.10261808574068e-3), (72, 6.102617056435018e-3), (73, 6.102618668961659e-3), (75, 6.102617784087809e-3)], main_patches: &[(&[26, 27, 28, 70], 6.1026180673475604e-3), (&[29, 30, 31, 72], 6.102617036055068e-3), (&[32, 33, 34, 73], 6.1026186503080025e-3), (&[35, 36, 37, 75], 6.102617764060949e-3)] },
    Row { name: "source-file-box_diff_cyl-all-edges", moved: true, main_volume: 950.648098797075, face_sides: 2, main_next_row_off: 3.7687186704715714e-11, main_seams: &[(32, 2.1476595554466467e-3), (33, 0e0), (34, 4.308606168864467e-3), (35, 4.307331512539287e-3), (36, 0e0), (37, 2.14766389173986e-3), (73, 6.102618668961659e-3), (75, 6.102617784087809e-3)], main_patches: &[(&[32, 33, 34, 73], 6.1026186503080025e-3), (&[35, 36, 37, 75], 6.102617764060949e-3)] },
    Row { name: "stubs3-union-inset-side-step-edges16-22-r1", moved: false, main_volume: 2601.7150940785264, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(30, 5.92321016970954e-3), (31, 4.94423282663257e-3), (32, 6.2344055212960395e-3), (37, 3.674735418723121e-5)], main_patches: &[(&[30, 31, 32, 37], 6.234405401424073e-3)] },
    Row { name: "stubs3-union-inset-side-step-edges17-22-r1", moved: false, main_volume: 2600.642084896182, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 5.923212927190241e-3), (30, 4.944232826340655e-3), (31, 6.234405521456313e-3), (37, 3.095368973196893e-5)], main_patches: &[(&[29, 30, 31, 37], 6.234405403092307e-3)] },
    Row { name: "stubs3-union-overhang-corner-edges14-22-r1", moved: false, main_volume: 2599.5690757124053, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(29, 5.923339907209535e-3), (30, 4.9442328269693955e-3), (31, 6.234405521563162e-3), (36, 3.63087639163081e-5)], main_patches: &[(&[29, 30, 31, 36], 6.234405400464064e-3)] },
    Row { name: "stubs3-union-overhang-corner-edges15-22-r1", moved: false, main_volume: 2598.710668410487, face_sides: 1, main_next_row_off: 3.832959412441994e-5, main_seams: &[(29, 5.923210169747027e-3), (30, 4.944232827508315e-3), (31, 6.234405521527545e-3), (36, 1.16103030300282e-4)], main_patches: &[(&[29, 30, 31, 36], 6.2344053972998295e-3)] },
    Row { name: "stubs3-union-overhang-front-edges14-22-r1", moved: false, main_volume: 2598.7106683663765, face_sides: 1, main_next_row_off: 3.8329594127972655e-5, main_seams: &[(29, 5.9232092991389805e-3), (30, 4.94423282703676e-3), (31, 6.234405521189191e-3), (36, 3.3773776877987224e-5)], main_patches: &[(&[29, 30, 31, 36], 6.234405403056393e-3)] },
    Row { name: "stubs4-Heart-all-edges-r0.1", moved: false, main_volume: 326.05828850759485, face_sides: 2, main_next_row_off: 1.609444153816142e-5, main_seams: &[(8, 5.834382971169943e-4), (9, 6.771257499057234e-4), (10, 1.7263064311850678e-3), (11, 6.770965791146431e-4), (12, 6.771191332215269e-4), (13, 1.5194066439734085e-3), (14, 6.770965794425795e-4), (15, 5.834382967364149e-4)], main_patches: &[(&[8, 9, 10, 11], 1.7263064088143519e-3), (&[12, 13, 14, 15], 1.519406643967111e-3)] },
];

/// One test per row, so the test runner spreads them over its threads.
macro_rules! row_tests {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check($name);
            }
        )*

        /// The rows the generated tests take, in [`ROWS`]'s order.
        const ROW_TESTS: &[&str] = &[$($name),*];
    };
}

row_tests! {
    corner3x2_union_inset_side_step_edges1_14_r1 => "corner3x2-union-inset-side-step-edges1-14-r1",
    corner3x2_union_inset_side_step_edges1_18_r1 => "corner3x2-union-inset-side-step-edges1-18-r1",
    corner3x2_union_inset_side_step_edges12_13_r1 => "corner3x2-union-inset-side-step-edges12-13-r1",
    corner3x2_union_inset_side_step_edges12_17_r1 => "corner3x2-union-inset-side-step-edges12-17-r1",
    corner3x2_union_inset_side_step_edges13_19_r1 => "corner3x2-union-inset-side-step-edges13-19-r1",
    corner3x2_union_inset_side_step_edges14_27_r1 => "corner3x2-union-inset-side-step-edges14-27-r1",
    corner3x2_union_inset_side_step_edges15_19_r1 => "corner3x2-union-inset-side-step-edges15-19-r1",
    corner3x2_union_inset_side_step_edges15_27_r1 => "corner3x2-union-inset-side-step-edges15-27-r1",
    corner3x2_union_overhang_corner_edges6_12_r1 => "corner3x2-union-overhang-corner-edges6-12-r1",
    corner3x2_union_overhang_corner_edges6_13_r1 => "corner3x2-union-overhang-corner-edges6-13-r1",
    corner3x2_union_overhang_front_edges15_22_r1 => "corner3x2-union-overhang-front-edges15-22-r1",
    corner3x2_union_overhang_front_edges15_24_r1 => "corner3x2-union-overhang-front-edges15-24-r1",
    curve_sih_box_diff_cyl_all_edges_r0_1 => "curve-sih-box_diff_cyl-all-edges-r0.1",
    curve_sih_box_diff_cyl_all_edges_r0_25 => "curve-sih-box_diff_cyl-all-edges-r0.25",
    curve_sih_box_diff_cyl_all_edges_r0_5 => "curve-sih-box_diff_cyl-all-edges-r0.5",
    curve_sih_box_diff_cyl_all_edges_r1_0 => "curve-sih-box_diff_cyl-all-edges-r1.0",
    curve_sih_box_diff_cyl_all_edges_step2 => "curve-sih-box_diff_cyl-all-edges-step2",
    source_file_box_diff_cyl_all_edges => "source-file-box_diff_cyl-all-edges",
    stubs3_union_inset_side_step_edges16_22_r1 => "stubs3-union-inset-side-step-edges16-22-r1",
    stubs3_union_inset_side_step_edges17_22_r1 => "stubs3-union-inset-side-step-edges17-22-r1",
    stubs3_union_overhang_corner_edges14_22_r1 => "stubs3-union-overhang-corner-edges14-22-r1",
    stubs3_union_overhang_corner_edges15_22_r1 => "stubs3-union-overhang-corner-edges15-22-r1",
    stubs3_union_overhang_front_edges14_22_r1 => "stubs3-union-overhang-front-edges14-22-r1",
    stubs4_heart_all_edges_r0_1 => "stubs4-Heart-all-edges-r0.1",
}

/// Every row of [`ROWS`] has exactly one test.
#[test]
fn every_row_has_its_own_test() {
    let rows: Vec<&str> = ROWS.iter().map(|row| row.name).collect();
    assert_eq!(rows, ROW_TESTS);
}
