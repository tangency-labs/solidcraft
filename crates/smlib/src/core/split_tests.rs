//! Split Face and Split Body on solidcore, against closed forms: face, edge and vertex counts,
//! volumes, how many faces each chosen face became, and every result exports to a record that
//! validates.
//!
//! The body is a 10 mm cube [0, 10]^3 on solidcore; the tools are described as
//! [`crate::ssi::Surface`] describes them.

use super::split::{split_body, split_body_by_sheet, split_faces};
use super::{Body, area, counts, make_box, to_arrays, volume};
use crate::Error;
use crate::export::native;
use crate::ssi::Spec;
use solidcore_ops::sweep::{Curve, SweepFrame, sweep_placed};
use solidcore_record::brep::{Face, Surface};

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn cube() -> Body {
    make_box([0.0; 3], [10.0; 3]).unwrap()
}

/// The plane x = `at` over u (along y) and v (along z) in `[-1, 11]`: it reaches over the cube.
fn plane_x(at: f64) -> Spec {
    Spec::Plane { origin: [at, 0.0, 0.0], xaxis: Y, yaxis: Z, u: [-1.0, 11.0], v: [-1.0, 11.0] }
}

/// The cylinder of radius 3 about the vertical line through (5, 5), from z = -1 to 11.
fn cylinder() -> Spec {
    Spec::Cylinder { origin: [5.0, 5.0, 0.0], xaxis: X, yaxis: Y, radius: 3.0, angle_deg: [0.0, 360.0], height: [-1.0, 11.0] }
}

#[derive(Clone, Debug)]
enum Op {
    /// Split the faces (all six when empty) by the tools.
    Faces(Vec<usize>, Vec<Spec>),
    Body(Spec),
}

#[derive(Clone, Debug, PartialEq)]
enum Expect {
    /// Volume, counts, and the total of faces made.
    Split(f64, [usize; 3], usize),
    /// The two sides' volumes and counts.
    Halves(f64, [usize; 3], f64, [usize; 3]),
    NoCut,
}

fn case(name: &str) -> (Op, Expect) {
    let all: Vec<usize> = (0..6).collect();
    match name {
        // Four faces cross x = 5, each cut in two; the four edges along x are cut at it.
        "split_faces_by_a_plane" => (Op::Faces(all, vec![plane_x(5.0)]), Expect::Split(1000.0, [10, 20, 12], 4)),
        // The cylinder meets the top and the bottom in a circle each: a disc and a holed face.
        "split_faces_by_a_cylinder" => (Op::Faces(all, vec![cylinder()]), Expect::Split(1000.0, [8, 14, 10], 2)),
        // Two planes in turn, the second cutting the first's pieces.
        "split_faces_by_two_planes" => (Op::Faces(all, vec![plane_x(3.0), plane_x(7.0)]), Expect::Split(1000.0, [14, 28, 16], 8)),
        "split_faces_missed_by_the_tool" => (Op::Faces(all, vec![plane_x(20.0)]), Expect::NoCut),
        "split_body_by_a_plane" => (Op::Body(plane_x(3.0)), Expect::Halves(300.0, [6, 12, 8], 700.0, [6, 12, 8])),
        // A tool that misses leaves both parts the whole body.
        "split_body_missed_by_the_tool" => (Op::Body(plane_x(20.0)), Expect::Halves(1000.0, [6, 12, 8], 1000.0, [6, 12, 8])),
        _ => panic!("no case {name}"),
    }
}

#[derive(Debug, PartialEq)]
enum Measured {
    Split { volume: f64, counts: [usize; 3], made: Vec<usize> },
    Halves { volumes: [f64; 2], counts: [[usize; 3]; 2] },
    NoCut,
}

type Got = std::result::Result<Measured, Error>;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

fn valid(name: &str, b: &Body) {
    let (rec, _) = native(&to_arrays(b).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
    solidcore_ops::record_check::validate(&rec).unwrap_or_else(|e| panic!("{name}: record: {e}"));
}

fn judge(name: &str, who: &str, got: Got) -> Got {
    let (_, expect) = case(name);
    match (&expect, &got) {
        (_, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (Expect::NoCut, Ok(m)) => assert_eq!(*m, Measured::NoCut, "{name}: {who}"),
        (Expect::Split(v, n, total), Ok(Measured::Split { volume, counts, made })) => {
            assert!(rel(*volume, *v) < 1e-9, "{name}: {who} volume {volume} vs analytic {v}");
            assert_eq!(counts, n, "{name}: {who} counts");
            assert_eq!(made.iter().sum::<usize>(), *total, "{name}: {who} faces made {made:?}");
        }
        (Expect::Halves(v0, n0, v1, n1), Ok(Measured::Halves { volumes, counts })) => {
            assert!(rel(volumes[0], *v0) < 1e-9 && rel(volumes[1], *v1) < 1e-9, "{name}: {who} volumes {volumes:?} vs {v0} {v1}");
            assert_eq!(counts, &[*n0, *n1], "{name}: {who} counts");
        }
        (e, Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
    }
    got
}

fn solidcore_case(name: &str) -> Got {
    let (op, _) = case(name);
    let got = match &op {
        Op::Faces(faces, tools) => {
            let tools: Vec<&Spec> = tools.iter().collect();
            split_faces(&cube(), faces, &tools).map(|r| match r {
                None => Measured::NoCut,
                Some((b, made)) => {
                    valid(name, &b);
                    Measured::Split { volume: volume(&b, 1e-8).unwrap(), counts: counts(&b).unwrap(), made }
                }
            })
        }
        Op::Body(tool) => split_body(&cube(), tool).map(|r| match r {
            None => Measured::NoCut,
            Some((b, f)) => {
                valid(name, &b);
                valid(name, &f);
                Measured::Halves {
                    volumes: [volume(&b, 1e-8).unwrap(), volume(&f, 1e-8).unwrap()],
                    counts: [counts(&b).unwrap(), counts(&f).unwrap()],
                }
            }
        }),
    };
    judge(name, "solidcore", got)
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
    split_faces_by_a_plane,
    split_faces_by_a_cylinder,
    split_faces_by_two_planes,
    split_faces_missed_by_the_tool,
    split_body_by_a_plane,
    split_body_missed_by_the_tool,
);

/// The capi's argument checks, before any kernel call.
#[test]
fn hostile_arguments_are_refused() {
    let b = cube();
    let p = plane_x(5.0);
    assert_eq!(split_faces(&b, &[], &[&p]).err(), Some(Error::Argument), "no face");
    assert_eq!(split_faces(&b, &[0], &[]).err(), Some(Error::Argument), "no tool");
    assert_eq!(split_faces(&b, &[0, 0], &[&p]).err(), Some(Error::Argument), "a face twice");
    assert_eq!(split_faces(&b, &[6], &[&p]).err(), Some(Error::Argument), "no face 6");
    assert_eq!(split_faces(&b, &[usize::MAX], &[&p]).err(), Some(Error::Argument), "no face usize::MAX");
    let many = vec![&p; super::split::MAX_TOOLS + 1];
    assert_eq!(split_faces(&b, &[0], &many).err(), Some(Error::Argument), "too many tools");
    let faces: Vec<usize> = (0..=super::split::MAX_FACES).collect();
    assert_eq!(split_faces(&b, &faces, &[&p]).err(), Some(Error::Argument), "too many faces");
}

/// The one-face sheet [0, 10]^2 on z = 0: the line along x from the origin swept 10 along y.
fn square_sheet() -> Body {
    let line = |end: [f64; 3]| Curve::Line { start: [0.0; 3], end };
    sweep_placed(&[line([10.0, 0.0, 0.0])], &[line([0.0, 10.0, 0.0])], SweepFrame::Follow, false).unwrap().0
}

/// A sheet is no solid to split in two (`SMLIB_ERR_NOT_SOLID`).
#[test]
fn a_sheet_is_not_split_into_two_bodies() {
    let sheet = square_sheet();
    assert!(rel(area(&sheet, 1e-8).unwrap(), 100.0) < 1e-9);
    assert_eq!(split_body(&sheet, &plane_x(5.0)).err(), Some(Error::NotSolid));
}

/// A tool that runs into a face and ends inside it leaves a crack in it, which cuts nothing off:
/// no face is split, and the call says the tool stops short, not that it missed. The strip of
/// the plane x = 5 over y in [-1, 5] enters the square sheet at its edge y = 0 and ends at y = 5;
/// over y in [-1, 11] the strip crosses the sheet and cuts it in two; at x = 20 it misses it.
#[test]
fn a_tool_that_ends_inside_the_face_stops_short() {
    let sheet = square_sheet();
    let strip = |at: f64, to: f64| Spec::Plane { origin: [at, 0.0, 0.0], xaxis: Y, yaxis: Z, u: [-1.0, to], v: [-1.0, 1.0] };
    assert_eq!(split_faces(&sheet, &[0], &[&strip(5.0, 5.0)]).err(), Some(Error::ToolStopsShort));
    let (cut, made) = split_faces(&sheet, &[0], &[&strip(5.0, 11.0)]).unwrap().expect("the strip crosses the sheet");
    valid("crossed sheet", &cut);
    assert_eq!(made, [1]);
    assert!(split_faces(&sheet, &[0], &[&strip(20.0, 11.0)]).unwrap().is_none(), "a strip that misses the sheet");
}

/// A sheet is no solid to split by a closed tool either: the whole sphere's split (the body's
/// intersection and difference with the ball) checks the body first, as the slice does (#14214).
#[test]
fn a_sheet_is_not_split_by_a_whole_sphere() {
    let sheet = square_sheet();
    let ball = Spec::Sphere { center: [5.0, 5.0, 0.0], xaxis: X, yaxis: Y, radius: 3.0, lon_deg: [0.0, 360.0], lat_deg: [-90.0, 90.0] };
    assert_eq!(split_body(&sheet, &ball).err(), Some(Error::NotSolid));
}

/// Faces of the box -20..5 x -20..20 x -20..5 left as a sheet (its other faces taken away): the
/// top z = 5 and, with `side`, the side x = 5 too.
fn lid(side: bool) -> Body {
    let slab = make_box([-20.0; 3], [25.0, 40.0, 25.0]).unwrap();
    let (rec, order) = slab.to_record().unwrap();
    // the plane face lying in `axis` = 5
    let at_5 = |f: &Face, axis: usize| matches!(&f.surface, Surface::Plane { origin, u_axis, v_axis } if origin[axis] == 5.0 && u_axis[axis] == 0.0 && v_axis[axis] == 0.0);
    let gone: Vec<usize> = (0..rec.faces.len()).filter(|&i| !(at_5(&rec.faces[i], 2) || (side && at_5(&rec.faces[i], 0)))).collect();
    assert_eq!(gone.len(), if side { 4 } else { 5 });
    solidcore_ops::trim::remove_faces(&slab, &order, &gone).unwrap()
}

/// The capi's test of `smlib_split_body_by_sheet` (fork 2fc375f): a cube 0..10 cut by an L of two
/// faces: the corner x < 5, z < 5 (250) on one side, the rest (750) on the other, whichever face
/// the sides are read from; on C++ too.
#[test]
fn split_body_by_an_l_sheet() {
    let (c, sheet) = (cube(), lid(true));
    for face in 0..2 {
        let (b, f) = split_body_by_sheet(&c, &sheet, face).unwrap().expect("the sheet divides the cube");
        valid("L sheet", &b);
        valid("L sheet", &f);
        let (vb, vf) = (volume(&b, 1e-8).unwrap(), volume(&f, 1e-8).unwrap());
        assert!(rel(vb.min(vf), 250.0) < 1e-9 && rel(vb.max(vf), 750.0) < 1e-9, "face {face}: {vb} + {vf}");
    }
}

/// What `smlib_split_body_by_sheet` refuses: a body that is no sheet, a face out of range, the
/// sheet as its own body; a face that ends inside the cube only nicks it, which is no cut.
#[test]
fn split_by_a_sheet_refuses() {
    let (c, sheet) = (cube(), lid(true));
    assert_eq!(split_body_by_sheet(&c, &make_box([-20.0; 3], [25.0, 40.0, 25.0]).unwrap(), 0).err(), Some(Error::Argument));
    assert_eq!(split_body_by_sheet(&c, &sheet, 2).err(), Some(Error::Argument));
    assert_eq!(split_body_by_sheet(&c, &c, 0).err(), Some(Error::Argument));
    assert!(split_body_by_sheet(&c, &lid(false), 0).unwrap().is_none());
    assert_eq!(split_body_by_sheet(&sheet, &lid(false), 0).err(), Some(Error::NotSolid));
}

/// The body split is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let b = cube();
    let _ = split_faces(&b, &[0, 1, 2, 3, 4, 5], &[&plane_x(5.0)]).unwrap();
    let _ = split_body(&b, &plane_x(5.0)).unwrap();
    assert_eq!(counts(&b).unwrap(), [6, 12, 8]);
    assert!(rel(volume(&b, 1e-8).unwrap(), 1000.0) < 1e-12);
}

/// A one-face sheet on a B-spline surface (as SolidCraft's extrude of a sketch spline makes it:
/// the cubic on (0, 0), (13, -8), (27, 8), (40, 0) in x and z, ruled along y over [0, 40]) cut by four plane strips standing on the square [12, 28]^2: the strips meet the
/// face inside its bounds, so the face's surface is intersected through its face's trimmed
/// cache. The square comes out as a face of its own, the sheet holed by it.
#[test]
fn a_spline_sheet_is_cut_by_strips_inside_it() {
    // The curve's four control points at y = 0 and at y = 40, ruled between.
    let curve = [[0.0, 0.0], [13.0, -8.0], [27.0, 8.0], [40.0, 0.0]];
    let at = |y: f64| solidcore_record::brep::Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: curve.iter().map(|[x, z]| [*x, y, *z]).collect(),
    };
    let rec = solidcore_ops::surface::ruled(&at(0.0), &at(40.0), false).unwrap();
    let sheet = Body::from_record(&rec, true).unwrap().0;
    assert_eq!(counts(&sheet).unwrap(), [1, 4, 4]);
    let whole = area(&sheet, 1e-8).unwrap();
    // The strip standing on the line from `o` along `d` (16 long), reaching past the sheet in z.
    let strip = |o: [f64; 3], d: [f64; 3]| Spec::Plane { origin: o, xaxis: Z, yaxis: d, u: [-60.0, 60.0], v: [0.0, 16.0] };
    let tools = [
        strip([12.0, 12.0, 0.0], X),
        strip([28.0, 12.0, 0.0], Y),
        strip([28.0, 28.0, 0.0], [-1.0, 0.0, 0.0]),
        strip([12.0, 28.0, 0.0], [0.0, -1.0, 0.0]),
    ];
    let tools: Vec<&Spec> = tools.iter().collect();
    let (cut, made) = split_faces(&sheet, &[0], &tools).unwrap().expect("the strips cut the sheet");
    valid("spline sheet", &cut);
    // The square's face and the holed sheet: four edges and vertices round each.
    assert_eq!(counts(&cut).unwrap(), [2, 8, 8]);
    assert_eq!(made, [1]);
    // Cutting moves no surface: the pieces cover the sheet.
    assert!(rel(area(&cut, 1e-8).unwrap(), whole) < 1e-9);
}
