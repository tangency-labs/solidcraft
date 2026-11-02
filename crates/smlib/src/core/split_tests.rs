//! Split Face and Split Body on solidcore, against closed forms: face, edge and vertex counts,
//! volumes, how many faces each chosen face became, and every result exports to a record that
//! validates. With SMLib built, the same input converted to the C++ kernel and the same tools
//! give the same result or the same error (parity only: the expected values are the analytic
//! ones).
//!
//! The body is a 10 mm cube [0, 10]^3 on solidcore; the tools are described as
//! [`crate::ssi::Surface`] describes them.

use super::split::{split_body, split_body_by_sheet, split_faces};
use super::{Body, area, counts, make_box, to_arrays, volume};
use crate::Error;
use crate::export::native;
use crate::ssi::Spec;

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
    rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
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

/// The C++ kernel's surface for `spec`.
#[cfg(feature = "native")]
fn surface(spec: &Spec) -> crate::ssi::Surface {
    use crate::ssi::Surface;
    match spec {
        Spec::Plane { origin, xaxis, yaxis, u, v } => Surface::plane(*origin, *xaxis, *yaxis, *u, *v),
        Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
            Surface::cylinder(*origin, *xaxis, *yaxis, *radius, *angle_deg, *height)
        }
        _ => panic!("no such tool in these cases"),
    }
    .unwrap()
}

/// The case on the C++ kernel, called directly (whatever the choice), on the cube converted
/// from solidcore's.
#[cfg(feature = "native")]
fn cpp_case(name: &str) -> Got {
    use crate::{Brep, error, ffi};
    crate::init().unwrap();
    let (op, _) = case(name);
    let body = Brep::made_core(cube());
    let measure = |h: &ffi::Handle| {
        let (rec, _) = native(&Brep::made(ffi::copy(h).unwrap()).cpp_arrays().unwrap()).unwrap();
        rec.validate().unwrap();
        (ffi::volume(h, 1e-8).unwrap(), ffi::counts(h).unwrap().map(|x| usize::try_from(x).unwrap()))
    };
    let got = match &op {
        Op::Faces(faces, tools) => {
            let made: Vec<crate::ssi::Surface> = tools.iter().map(surface).collect();
            let handles: Vec<&ffi::ssi::SurfaceHandle> = made.iter().map(|s| s.handle().unwrap()).collect();
            let faces: Vec<i64> = faces.iter().map(|&f| f as i64).collect();
            match ffi::split::split_faces(body.cpp().unwrap(), &faces, &handles) {
                Ok((h, made)) => {
                    let (volume, counts) = measure(&h);
                    Ok(Measured::Split { volume, counts, made: made.into_iter().map(|m| usize::try_from(m).unwrap()).collect() })
                }
                Err(ffi::ops::ERR_NO_CUT) => Ok(Measured::NoCut),
                Err(s) => Err(error(s)),
            }
        }
        Op::Body(tool) => match ffi::split::split_body(body.cpp().unwrap(), surface(tool).handle().unwrap()) {
            Ok((b, f)) => {
                let (vb, nb) = measure(&b);
                let (vf, nf) = measure(&f);
                Ok(Measured::Halves { volumes: [vb, vf], counts: [nb, nf] })
            }
            Err(ffi::ops::ERR_NO_CUT) => Ok(Measured::NoCut),
            Err(s) => Err(error(s)),
        },
    };
    judge(name, "C++", got)
}

#[cfg(feature = "native")]
fn parity(name: &str) {
    match (solidcore_case(name), cpp_case(name)) {
        (Ok(Measured::Split { volume: a, counts: na, made: ma }), Ok(Measured::Split { volume: b, counts: nb, made: mb })) => {
            assert!(rel(a, b) < 1e-9, "{name}: solidcore {a} vs C++ {b}");
            assert_eq!(na, nb, "{name}: counts (solidcore vs C++)");
            assert_eq!(ma, mb, "{name}: faces made per face (solidcore vs C++)");
        }
        (Ok(Measured::Halves { volumes: a, counts: na }), Ok(Measured::Halves { volumes: b, counts: nb })) => {
            assert!(rel(a[0], b[0]) < 1e-9 && rel(a[1], b[1]) < 1e-9, "{name}: solidcore {a:?} vs C++ {b:?}");
            assert_eq!(na, nb, "{name}: counts (solidcore vs C++)");
        }
        (a, b) => assert_eq!(a, b, "{name}: solidcore vs C++"),
    }
}

macro_rules! cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_closed_form() {
                let _ = super::solidcore_case(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                super::parity(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            fn the_cpp_kernel_matches_the_closed_form() {
                let _cpp = crate::route::on_cpp();
                let _ = super::cpp_case(stringify!($m));
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

/// A sheet is no solid to split in two (`SMLIB_ERR_NOT_SOLID`).
#[test]
fn a_sheet_is_not_split_into_two_bodies() {
    use solidcore::curve::curve::Curve;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::sweep::api_brep::create_linear_sweep;
    let sheet = super::run("sheet", |k| {
        let c = Curve::from(create_line_segment(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0))?);
        create_linear_sweep(k, &[Some(&c)], Vec3::new(0.0, 1.0, 0.0), 10.0, false)
    })
    .map(Body::new)
    .unwrap();
    assert!(rel(area(&sheet, 1e-8).unwrap(), 100.0) < 1e-9);
    assert_eq!(split_body(&sheet, &plane_x(5.0)).err(), Some(Error::NotSolid));
}

/// Faces of the box -20..5 x -20..20 x -20..5 copied off it into a sheet (`CopyFaces` with merge,
/// as the capi's test): the top z = 5 and, with `side`, the side x = 5 too.
fn lid(side: bool) -> Body {
    use solidcore::EntityRef;
    use solidcore::geom::tol::USE_DEFAULT;
    use solidcore::query::api_queries::face_internal_point;
    use solidcore::topology::brep::p1::faces;
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::brep::p3::new_brep;
    use solidcore::topology::brep::p7::copy_faces;
    let slab = make_box([-20.0; 3], [25.0, 40.0, 25.0]).unwrap();
    super::run("sheet", |k| {
        let mut whole = solidcore::topology::brep::p3::brep_copy(k, &slab.0.lock().unwrap())?;
        let mut picked = Vec::new();
        for f in faces(&whole, None)? {
            let p = face_internal_point(k, &mut whole, f)?;
            if (p.z - 5.0).abs() < 1e-9 || (side && (p.x - 5.0).abs() < 1e-9) {
                picked.push(f);
            }
        }
        assert_eq!(picked.len(), if side { 2 } else { 1 });
        let mut w = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
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
    .unwrap()
}

/// The capi's test of `smlib_split_body_by_sheet` (fork 2fc375f): a cube 0..10 cut by an L of two
/// faces: the corner x < 5, z < 5 (250) on one side, the rest (750) on the other, whichever face
/// the sides are read from; on C++ too.
#[test]
fn split_body_by_an_l_sheet() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    let (c, sheet) = (cube(), lid(true));
    for face in 0..2 {
        let (b, f) = split_body_by_sheet(&c, &sheet, face).unwrap().expect("the sheet divides the cube");
        valid("L sheet", &b);
        valid("L sheet", &f);
        let (vb, vf) = (volume(&b, 1e-8).unwrap(), volume(&f, 1e-8).unwrap());
        assert!(rel(vb.min(vf), 250.0) < 1e-9 && rel(vb.max(vf), 750.0) < 1e-9, "face {face}: {vb} + {vf}");
        #[cfg(feature = "native")]
        {
            let (body, tool) = (crate::Brep::made_core(cube()), crate::Brep::made_core(lid(true)));
            let (bc, fc) = crate::ffi::split::split_body_by_sheet(body.cpp().unwrap(), tool.cpp().unwrap(), face as i64).unwrap();
            let (vbc, vfc) = (crate::ffi::volume(&bc, 1e-8).unwrap(), crate::ffi::volume(&fc, 1e-8).unwrap());
            assert!(rel(vb, vbc) < 1e-9 && rel(vf, vfc) < 1e-9, "face {face}: {vb} + {vf} vs C++ {vbc} + {vfc}");
        }
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

/// Face and edge tags (`Brep::tagged`) through a face split on solidcore, as on C++.
#[cfg(feature = "native")]
mod history {
    use crate::{Brep, ffi};

    #[test]
    fn tags_cross_a_face_split() {
        let _cpp = crate::route::on_cpp();
        crate::init().unwrap();
        let a = Brep::made_core(super::cube()).tagged(0).unwrap();
        let p = super::plane_x(5.0);
        let s = super::surface(&p);
        let faces = [0i64, 1, 2, 3, 4, 5];
        let cpp = Brep::made(ffi::split::split_faces(a.cpp().unwrap(), &faces, &[s.handle().unwrap()]).unwrap().0).history().unwrap();
        let core = super::split_faces(a.core().unwrap(), &[0, 1, 2, 3, 4, 5], &[&p]).unwrap().unwrap().0;
        let core = Brep::made_core(core).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }
}

/// A one-face sheet on a B-spline surface (as SolidCraft's extrude of a sketch spline makes it:
/// the cubic through (0, 0), (13, -8), (27, 8), (40, 0) in x and z, straight along y over
/// [0, 40]) cut by four plane strips standing on the square [12, 28]^2: the strips meet the
/// face inside its bounds, so the face's surface is intersected through its face's trimmed
/// cache. The square comes out as a face of its own, the sheet holed by it.
#[test]
fn a_spline_sheet_is_cut_by_strips_inside_it() {
    use solidcore::geom::tol::USE_DEFAULT;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::nurbs::b_spline_curve::p1::KnotType;
    use solidcore::nurbs::b_spline_surface::p1::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
    use solidcore::surface::surface::Surface;
    use solidcore::topology::brep::p3::new_brep;
    use solidcore::topology::brep::p8::create_face_from_surface;
    // Control points u-major: the curve's four, each at y = 0 and y = 40.
    let curve = [[0.0, 0.0], [13.0, -8.0], [27.0, 8.0], [40.0, 0.0]];
    let pts: Vec<Vec3> = curve.iter().flat_map(|[x, z]| [Vec3::new(*x, 0.0, *z), Vec3::new(*x, 40.0, *z)]).collect();
    let sheet = super::run("sheet", |k| {
        let surface = Surface::from(BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 1,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[4, 4],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 40.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })?);
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        let natural = surface.natural_uv_domain_with_nurb(k)?;
        super::sweep::build_faces(&mut b, |s, fenv| create_face_from_surface(k, s, Some(surface), &natural, fenv))?;
        b.refresh_cached_topology();
        Ok(b)
    })
    .map(Body::new)
    .unwrap();
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
