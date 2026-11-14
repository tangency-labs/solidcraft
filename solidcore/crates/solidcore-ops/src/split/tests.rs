//! Split Face and Split Body against closed forms (moved from SolidCraft's split tests): face,
//! edge and vertex counts, volumes and areas, how many faces each chosen face became; and the
//! corpus's cases (`crates/solidcore/tests/kernel_cases/`) against the values SolidCraft's rig
//! measured.
// The closed forms use pi and square roots; the tests compute them, not ops.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;
use crate::make_box;
use crate::test_corpus::{area, entry, kernel_order, operand, rel, rig, rig_fixes, valid};
use solidcore_record::brep::Frame;

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

/// The 10 mm cube [0, 10]^3 and its order (the kernel's own lists).
fn cube() -> (Body, Order) {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let o = kernel_order(&b);
    (b, o)
}

fn plane(origin: [f64; 3], u_axis: [f64; 3], v_axis: [f64; 3], u: [f64; 2], v: [f64; 2]) -> Tool {
    Tool {
        surface: RecordSurface::Plane {
            origin,
            u_axis,
            v_axis,
        },
        domain: [u, v],
    }
}

/// The plane x = `at` over u (along y) and v (along z) in `[-1, 11]`: it reaches over the cube.
fn plane_x(at: f64) -> Tool {
    plane([at, 0.0, 0.0], Y, Z, [-1.0, 11.0], [-1.0, 11.0])
}

fn cylinder(
    origin: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    radius: f64,
    angle_deg: [f64; 2],
    height: [f64; 2],
) -> Tool {
    Tool {
        surface: RecordSurface::Cylinder {
            frame: Frame {
                origin,
                x_axis,
                y_axis,
            },
            radius,
        },
        domain: [angle_deg.map(f64::to_radians), height],
    }
}

/// The cylinder of radius 3 about the vertical line through (5, 5), from z = -1 to 11.
fn cylinder_3() -> Tool {
    cylinder([5.0, 5.0, 0.0], X, Y, 3.0, [0.0, 360.0], [-1.0, 11.0])
}

/// The corpus's tool (SolidCraft's spec of it, angles in degrees) as a record tool: a plane's
/// axes are unit in every case, so its parameters are the record's.
fn corpus_tool(t: &serde_json::Value) -> Tool {
    let v3 = |v: &serde_json::Value| -> [f64; 3] { [0, 1, 2].map(|i| v[i].as_f64().unwrap()) };
    let v2 = |v: &serde_json::Value| -> [f64; 2] { [0, 1].map(|i| v[i].as_f64().unwrap()) };
    if let Some(p) = t.get("plane") {
        let (x, y) = (v3(&p["xaxis"]), v3(&p["yaxis"]));
        for a in [x, y] {
            assert!((a[0].hypot(a[1]).hypot(a[2]) - 1.0).abs() < 1e-15, "{a:?}");
        }
        return plane(v3(&p["origin"]), x, y, v2(&p["u"]), v2(&p["v"]));
    }
    let c = &t["cylinder"];
    cylinder(
        v3(&c["origin"]),
        v3(&c["xaxis"]),
        v3(&c["yaxis"]),
        c["radius"].as_f64().unwrap(),
        v2(&c["angle_deg"]),
        v2(&c["height"]),
    )
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

#[test]
fn faces_by_a_plane() {
    let (b, o) = cube();
    let all: Vec<usize> = (0..6).collect();
    // four faces cross x = 5, each cut in two; the four edges along x are cut at it
    let (r, made) = split_faces(&b, &o, &all, &[plane_x(5.0)]).unwrap().unwrap();
    valid("plane", &r);
    assert!(rel(volume(&r), 1000.0) < 1e-9);
    assert_eq!(r.counts().unwrap(), [10, 20, 12]);
    assert_eq!(made.iter().sum::<usize>(), 4);
}

#[test]
fn faces_by_a_cylinder() {
    let (b, o) = cube();
    let all: Vec<usize> = (0..6).collect();
    // the cylinder meets the top and the bottom in a circle each: a disc and a holed face
    let (r, made) = split_faces(&b, &o, &all, &[cylinder_3()]).unwrap().unwrap();
    valid("cylinder", &r);
    assert!(rel(volume(&r), 1000.0) < 1e-9);
    assert_eq!(r.counts().unwrap(), [8, 14, 10]);
    assert_eq!(made.iter().sum::<usize>(), 2);
}

#[test]
fn faces_by_two_planes_in_turn() {
    let (b, o) = cube();
    let all: Vec<usize> = (0..6).collect();
    let (r, made) = split_faces(&b, &o, &all, &[plane_x(3.0), plane_x(7.0)])
        .unwrap()
        .unwrap();
    valid("two planes", &r);
    assert!(rel(volume(&r), 1000.0) < 1e-9);
    assert_eq!(r.counts().unwrap(), [14, 28, 16]);
    assert_eq!(made.iter().sum::<usize>(), 8);
}

#[test]
fn faces_missed_by_the_tool() {
    let (b, o) = cube();
    let all: Vec<usize> = (0..6).collect();
    assert!(split_faces(&b, &o, &all, &[plane_x(20.0)])
        .unwrap()
        .is_none());
}

#[test]
fn body_by_a_plane() {
    let (b, _) = cube();
    let (back, front) = split_body(&b, &plane_x(3.0)).unwrap().unwrap();
    valid("back", &back);
    valid("front", &front);
    // the plane's normal is +x: behind it x < 3
    assert!(rel(volume(&back), 300.0) < 1e-9 && rel(volume(&front), 700.0) < 1e-9);
    assert_eq!(back.counts().unwrap(), [6, 12, 8]);
    assert_eq!(front.counts().unwrap(), [6, 12, 8]);
}

/// A tool that misses leaves both parts the whole body.
#[test]
fn body_missed_by_the_tool() {
    let (b, _) = cube();
    let (back, front) = split_body(&b, &plane_x(20.0)).unwrap().unwrap();
    assert!(rel(volume(&back), 1000.0) < 1e-9 && rel(volume(&front), 1000.0) < 1e-9);
}

#[test]
fn bad_asks_are_refused() {
    let (b, o) = cube();
    let p = plane_x(5.0);
    let one = std::slice::from_ref(&p);
    assert_eq!(
        split_faces(&b, &o, &[], one).err(),
        Some(Error::Argument),
        "no face"
    );
    assert_eq!(
        split_faces(&b, &o, &[0], &[]).err(),
        Some(Error::Argument),
        "no tool"
    );
    assert_eq!(
        split_faces(&b, &o, &[0, 0], one).err(),
        Some(Error::Argument),
        "a face twice"
    );
    assert_eq!(
        split_faces(&b, &o, &[6], one).err(),
        Some(Error::Argument),
        "no face 6"
    );
    assert_eq!(
        split_faces(&b, &o, &[usize::MAX], one).err(),
        Some(Error::Argument)
    );
    let many = vec![p.clone(); MAX_TOOLS + 1];
    assert_eq!(
        split_faces(&b, &o, &[0], &many).err(),
        Some(Error::Argument),
        "too many tools"
    );
    let faces: Vec<usize> = (0..=MAX_FACES).collect();
    assert_eq!(
        split_faces(&b, &o, &faces, one).err(),
        Some(Error::Argument),
        "too many faces"
    );
    // a tool the kernel refuses: a box not increasing, axes not square
    let flat = plane([5.0, 0.0, 0.0], Y, Z, [1.0, 1.0], [0.0, 1.0]);
    let refused = split_body(&b, &flat).err();
    assert!(
        matches!(refused, Some(Error::Refused { reason, .. }) if !reason.is_empty()),
        "{refused:?}"
    );
    let skew = plane([5.0, 0.0, 0.0], Y, [0.0, 1.0, 1.0], [0.0, 1.0], [0.0, 1.0]);
    let refused = split_faces(&b, &o, &[0], &[skew]).err();
    assert!(
        matches!(refused, Some(Error::Refused { reason, .. }) if !reason.is_empty()),
        "{refused:?}"
    );
}

/// A sheet is no solid to split in two.
#[test]
fn a_sheet_is_not_split_into_two_bodies() {
    let (sheet, _) = lid(false);
    assert_eq!(
        split_body(&sheet, &plane_x(5.0)).err(),
        Some(Error::NotSolid)
    );
}

/// The body split is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let (b, o) = cube();
    let _ = split_faces(&b, &o, &[0, 1, 2, 3, 4, 5], &[plane_x(5.0)]).unwrap();
    let _ = split_body(&b, &plane_x(5.0)).unwrap();
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    assert!(rel(volume(&b), 1000.0) < 1e-12);
}

/// The top z = 5 of the box -20..5 x -20..20 x -20..5 and, with `side`, its side x = 5 too, as a
/// sheet: each face's surface over its face box, sewn where they meet.
fn lid(side: bool) -> (Body, Order) {
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::brep::p7::copy_faces;
    let slab = make_box([-20.0; 3], [25.0, 40.0, 25.0]).unwrap();
    let b = crate::session::run("lid", |k| {
        let mut whole = brep_copy(k, &slab.lock())?;
        let mut picked = Vec::new();
        for f in brep_p1::faces(&whole, None)? {
            let p = solidcore::query::api_queries::face_internal_point(k, &mut whole, f)?;
            if (p.z - 5.0).abs() < 1e-9 || (side && (p.x - 5.0).abs() < 1e-9) {
                picked.push(f);
            }
        }
        assert_eq!(picked.len(), if side { 2 } else { 1 });
        let mut w = solidcore::topology::brep::p3::new_brep(
            k,
            solidcore::geom::tol::USE_DEFAULT,
            solidcore::geom::tol::USE_DEFAULT,
        )?;
        w.set_tolerance(whole.tolerance());
        {
            let mut state = ArenaNotifyState::new(&w);
            let mut env = state.env(None);
            let key_of = env.key_of;
            let this_face = |f| key_of(EntityRef::Face(f));
            copy_faces(k, &mut whole, &picked, &mut w, true, &this_face, &mut env)?;
        }
        w.refresh_cached_topology();
        Ok(w)
    })
    .map(Body::new)
    .unwrap();
    let o = kernel_order(&b);
    (b, o)
}

/// A cube 0..10 cut by an L of two faces: the corner x < 5, z < 5 (250) on one side, the rest
/// (750) on the other, whichever face the sides are read from.
#[test]
fn body_by_an_l_sheet() {
    let (c, _) = cube();
    let (sheet, so) = lid(true);
    for face in 0..2 {
        let (b, f) = split_body_by_sheet(&c, &sheet, &so, face)
            .unwrap()
            .expect("the sheet divides the cube");
        valid("L sheet", &b);
        valid("L sheet", &f);
        let (vb, vf) = (volume(&b), volume(&f));
        assert!(
            rel(vb.min(vf), 250.0) < 1e-9 && rel(vb.max(vf), 750.0) < 1e-9,
            "face {face}: {vb} + {vf}"
        );
    }
}

/// A body that is no sheet, a face out of range, the sheet as its own body; a face that ends
/// inside the cube only nicks it, which is no cut.
#[test]
fn by_a_sheet_refuses() {
    let (c, co) = cube();
    let (sheet, so) = lid(true);
    let slab = make_box([-20.0; 3], [25.0, 40.0, 25.0]).unwrap();
    let slab_order = kernel_order(&slab);
    assert_eq!(
        split_body_by_sheet(&c, &slab, &slab_order, 0).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        split_body_by_sheet(&c, &sheet, &so, 2).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        split_body_by_sheet(&c, &c, &co, 0).err(),
        Some(Error::Argument)
    );
    let (nick, no) = lid(false);
    assert!(split_body_by_sheet(&c, &nick, &no, 0).unwrap().is_none());
    assert_eq!(
        split_body_by_sheet(&sheet, &nick, &no, 0).err(),
        Some(Error::NotSolid)
    );
}

/// A one-face sheet on a B-spline surface (the cubic through (0, 0), (13, -8), (27, 8),
/// (40, 0) in x and z, straight along y over [0, 40]) cut by four plane strips standing on the
/// square [12, 28]^2: the strips meet the face inside its bounds, so the face's surface is
/// intersected through its face's trimmed cache. The square comes out as a face of its own,
/// the sheet holed by it, and the pieces cover the sheet.
#[test]
fn a_spline_sheet_is_cut_by_strips_inside_it() {
    let curve = [[0.0, 0.0], [13.0, -8.0], [27.0, 8.0], [40.0, 0.0]];
    let points: Vec<Vec<[f64; 3]>> = curve
        .iter()
        .map(|[x, z]| vec![[*x, 0.0, *z], [*x, 40.0, *z]])
        .collect();
    let spline = RecordSurface::Spline {
        u_knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 40.0, 40.0],
        points,
    };
    let sheet = crate::session::run("sheet", |k| {
        let (s, _) = from_parameters(
            k,
            &solidcore_convert::surface(&spline).unwrap(),
            [[0.0, 1.0], [0.0, 40.0]],
        )?;
        Ok(sheet_on(k, s, 1e-6)?.0)
    })
    .map(Body::new)
    .unwrap();
    let so = kernel_order(&sheet);
    assert_eq!(sheet.counts().unwrap(), [1, 4, 4]);
    let whole = area(&sheet);
    // the strip standing on the line from `o` along `d` (16 long), reaching past the sheet in z
    let strip = |o: [f64; 3], d: [f64; 3]| plane(o, Z, d, [-60.0, 60.0], [0.0, 16.0]);
    let tools = [
        strip([12.0, 12.0, 0.0], X),
        strip([28.0, 12.0, 0.0], Y),
        strip([28.0, 28.0, 0.0], [-1.0, 0.0, 0.0]),
        strip([12.0, 28.0, 0.0], [0.0, -1.0, 0.0]),
    ];
    let (cut, made) = split_faces(&sheet, &so, &[0], &tools)
        .unwrap()
        .expect("the strips cut the sheet");
    valid("spline sheet", &cut);
    assert_eq!(cut.counts().unwrap(), [2, 8, 8]);
    assert_eq!(made, [1]);
    assert!(rel(area(&cut), whole) < 1e-9);
}

// The corpus's cases. Each runs the traced operation on the traced operand and checks what it
// makes against the rig's measured values: the volume and area summed over the case's result
// bodies (a body the case carries through unchanged added from its own record), and the face,
// edge and vertex counts where the result is the operation's own output.

/// A body the case makes with another family's operation, carried to its end unchanged: its
/// closed-form volume, area and counts.
struct Closed {
    volume: f64,
    area: f64,
    counts: [usize; 3],
}

/// A face split: the result body's volume, area and counts, with those of a body the case
/// carries through (`carried`, its closed form), are the rig's.
fn faces_case(case: &str, operand_name: &str, carried: Option<Closed>) {
    rig_fixes();
    let e = entry(case);
    let call = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "split_faces")
        .unwrap();
    let faces: Vec<usize> = call["params"]["faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_u64().unwrap() as usize)
        .collect();
    let tools: Vec<Tool> = call["params"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(corpus_tool)
        .collect();
    let (b, o) = operand(case, operand_name);
    let (r, _) = split_faces(&b, &o, &faces, &tools)
        .unwrap()
        .expect("the tool cuts the face");
    valid(case, &r);
    let m = rig(&e);
    let (mut v, mut a) = (volume(&r), area(&r));
    let mut counts = r.counts().unwrap();
    if let Some(c) = carried {
        v += c.volume;
        a += c.area;
        counts = [0, 1, 2].map(|i| counts[i] + c.counts[i]);
    }
    println!(
        "{case}: volume {v} (rig {}), area {a} (rig {}), counts {counts:?} (rig {:?})",
        m.volume, m.area, m.counts
    );
    assert!(
        rel(v, m.volume) < 1e-9,
        "{case}: volume {v} vs rig {}",
        m.volume
    );
    assert!(rel(a, m.area) < 1e-9, "{case}: area {a} vs rig {}", m.area);
    assert_eq!(counts, m.counts, "{case}: counts");
}

/// A block's top face split by a sketch circle on it: a disc face and the face around it; the
/// shape unchanged (closed form: V 12000, A 3800, 7 faces).
#[test]
fn corpus_split_face_by_a_sketch_circle() {
    faces_case("booleans/cov-split-face-by-a-sketch-circle", "b2", None);
}

/// A block's top face split by the side of a cylinder standing on it (closed form: the block
/// keeps V 12000, the cylinder 250 pi; 7 + 3 faces).
#[test]
fn corpus_split_face_by_another_bodys_face() {
    // the cutter, the extrude of the circle r 5 at (20, 15, 10) 10 up: a cylinder, its side
    // and two discs, one seam edge and two circles, a vertex on each circle
    let cutter = Closed {
        volume: 250.0 * std::f64::consts::PI,
        area: 150.0 * std::f64::consts::PI,
        counts: [3, 3, 2],
    };
    faces_case(
        "booleans/cov-split-face-by-another-bodys-face",
        "b3",
        Some(cutter),
    );
}

/// A body split: both parts' volume and area summed are the rig's (the rig's counts are after
/// a later merge of faces, so the parts' counts are checked against the corpus's record of the
/// part that merge took, `merged`, and the other part's counts are added).
fn body_case(
    case: &str,
    operand_name: &str,
    by_sheet: Option<&str>,
    carried: Option<&str>,
    merged: Option<&str>,
) {
    rig_fixes();
    let e = entry(case);
    let (b, _) = operand(case, operand_name);
    let parts = match by_sheet {
        None => {
            let call = e["calls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["op"] == "split_body")
                .unwrap();
            split_body(&b, &corpus_tool(&call["params"]["tool"])).unwrap()
        }
        Some(s) => {
            let call = e["calls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["op"] == "split_body_by_sheet")
                .unwrap();
            let face = call["params"]["face"].as_u64().unwrap() as usize;
            let (sheet, so) = operand(case, s);
            split_body_by_sheet(&b, &sheet, &so, face).unwrap()
        }
    };
    let (back, front) = parts.expect("the tool divides the body");
    valid(case, &back);
    valid(case, &front);
    let m = rig(&e);
    let (v, mut a) = (volume(&back) + volume(&front), area(&back) + area(&front));
    let mut counts = [0, 1, 2].map(|i| back.counts().unwrap()[i] + front.counts().unwrap()[i]);
    if let Some(c) = carried {
        // a sheet: an area and no volume
        let (cb, _) = operand(case, c);
        a += area(&cb);
        let n = cb.counts().unwrap();
        counts = [0, 1, 2].map(|i| counts[i] + n[i]);
    }
    println!(
        "{case}: volume {v} (rig {}), area {a} (rig {}), parts {:?} {:?} {} {}, counts {counts:?} (rig raw {:?})",
        m.volume,
        m.area,
        back.counts().unwrap(),
        front.counts().unwrap(),
        volume(&back),
        volume(&front),
        crate::test_corpus::raw_counts(&e),
    );
    assert!(
        rel(v, m.volume) < 1e-9,
        "{case}: volume {v} vs rig {}",
        m.volume
    );
    assert!(rel(a, m.area) < 1e-9, "{case}: area {a} vs rig {}", m.area);
    match merged {
        // the rig's counts are after a later merge of faces: the part it merged is the corpus's
        // record of it, the other part is in the rig's counts as it is
        Some(name) => {
            let (mb, _) = operand(case, name);
            let n = mb.counts().unwrap();
            let (cb, cf) = (back.counts().unwrap(), front.counts().unwrap());
            let other = if cb == n { cf } else { cb };
            assert!(
                cb == n || cf == n,
                "{case}: neither part {cb:?} {cf:?} is the record {n:?}"
            );
            let raw = crate::test_corpus::raw_counts(&e);
            assert!(
                (0..3).all(|i| raw[i] >= other[i]),
                "{case}: {other:?} past the rig's {raw:?}"
            );
            println!("{case}: merged part {n:?}, other {other:?}, rig raw {raw:?}");
        }
        None => assert_eq!(counts, crate::test_corpus::raw_counts(&e), "{case}: counts"),
    }
}

/// Split body where the splitting plane contains an end face (with a boss on it and an R1
/// fillet on the face's edge): two bodies, 17116.19 and 3434.0 (closed form 17117.44 within the
/// fillet's 1e-3).
#[test]
fn corpus_split_along_plane_of_a_face() {
    body_case(
        "booleans/3447-split-along-plane-of-a-face",
        "b17",
        None,
        None,
        Some("b23"),
    );
}

/// An L-shaped body split by its own face x = 15 extended through the base: 9000 + 5000.
#[test]
fn corpus_split_body_by_its_own_face_extended() {
    body_case(
        "booleans/4654-split-body-by-its-own-face-extended",
        "b12",
        None,
        None,
        Some("b18"),
    );
}

/// A thin-walled cup split by an extruded spline sheet extended at both ends: the two parts add
/// up to the cup (closed form 3349.04); the sheet stays, a third body.
#[test]
fn corpus_split_cup_by_extended_spline_surface() {
    body_case(
        "booleans/3830-split-cup-by-extended-spline-surface",
        "b25",
        Some("b24"),
        Some("b24"),
        None,
    );
}

/// A body whose face ids are not its copy's (made by a split, its face list then reversed, as
/// an importer may leave it): each record face is the body's own, not the one a renumbered
/// copy has under its id. The plane y = 5 cuts exactly the faces whose box straddles it.
#[test]
fn a_face_list_out_of_id_order_splits_the_faces_asked() {
    let (b, o) = cube();
    let all: Vec<usize> = (0..6).collect();
    let (r, _) = split_faces(&b, &o, &all, &[plane_x(5.0)]).unwrap().unwrap();
    {
        let mut g = r.lock();
        let mut faces = g.face_list().to_vec();
        faces.reverse();
        g.set_face_list(faces).unwrap();
    }
    let o = kernel_order(&r);
    let plane_y = plane([0.0, 5.0, 0.0], Z, X, [-1.0, 11.0], [-1.0, 11.0]);
    let k = Kernel::default();
    for (i, &f) in o.faces.iter().enumerate() {
        let bx = calculate_bounding_box(&k, &mut *r.lock(), f, false).unwrap();
        let crosses = bx.min().y < 5.0 - 1e-6 && bx.max().y > 5.0 + 1e-6;
        let made = split_faces(&r, &o, &[i], std::slice::from_ref(&plane_y))
            .unwrap()
            .map_or(0, |(_, made)| made[0]);
        assert_eq!(made, usize::from(crosses), "record face {i}: {bx:?}");
    }
}
