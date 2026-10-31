//! Untrim and extend on solidcore, against closed forms: areas and counts, and every result exports to a
//! record that validates. With SMLib built, the same input converted to the C++ kernel gives the
//! same result or the same error (parity only: the expected values are the analytic ones).
//!
//! An untrim's body is a 20 x 20 x 5 plate with a 3 mm through hole at (10, 10), made on
//! solidcore; its top face is a square with a round hole. An extend's is a one-face sheet that is
//! its surface's whole parameter box: a 10 mm square, or a quarter of a cylinder of radius 10 and
//! height 10.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::trim::{extend_sheet, untrim_face};
use super::{Body, area, counts, make_box, make_cylinder, to_arrays};
use crate::export::native;
use crate::trim::Continuation;
use crate::{BooleanOp, Error};

const TOL: f64 = 0.01;
/// A point on the top face, away from the hole.
const TOP: [f64; 3] = [2.0, 2.0, 5.0];
/// A point on the hole's rim on the top face.
const RIM: [f64; 3] = [13.0, 10.0, 5.0];

/// The plate's top as a sheet of its own: a 20 mm square at z = 0 with a 3 mm hole at (10, 10).
fn holed_sheet() -> Body {
    use crate::sweeps::Curve;
    let line = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let (a, b, c, d) = ([0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 20.0, 0.0], [0.0, 20.0, 0.0]);
    let hole = Curve::Arc { center: [10.0, 10.0, 0.0], axis: [0.0, 0.0, 1.0], reference: [1.0, 0.0, 0.0], radius: 3.0, start: 0.0, end: 2.0 * PI };
    super::sweep::planar_faces(&[line(a, b), line(b, c), line(c, d), line(d, a), hole]).unwrap()
}

/// On the sheet: a point away from the hole, and one on the hole's rim.
const SHEET: [f64; 3] = [2.0, 2.0, 0.0];
const SHEET_RIM: [f64; 3] = [13.0, 10.0, 0.0];

#[derive(Clone, Copy, Debug)]
enum Input {
    Plate,
    Sheet,
}

impl Input {
    fn core(self) -> Body {
        match self {
            Input::Plate => plate(),
            Input::Sheet => holed_sheet(),
        }
    }
}

fn plate() -> Body {
    let block = make_box([0.0; 3], [20.0, 20.0, 5.0]).unwrap();
    let pin = make_cylinder([10.0, 10.0, -1.0], 3.0, 7.0).unwrap();
    super::boolean::boolean(&block, &pin, BooleanOp::Subtract).unwrap().unwrap()
}

#[derive(Clone, Copy, Debug)]
struct Case {
    input: Input,
    point: [f64; 3],
    /// `None`: every hole.
    holes: Option<&'static [[f64; 3]]>,
    outer: bool,
    in_place: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Expect {
    /// A sheet: its area and face count.
    Sheet(f64, usize),
    Fails(Error),
}

fn case(name: &str) -> (Case, Expect) {
    let c = |input, point, holes, outer, in_place| Case { input, point, holes, outer, in_place };
    let square = 400.0;
    let holed = 400.0 - 9.0 * PI;
    let (plate, sheet) = (Input::Plate, Input::Sheet);
    match name {
        // The plate's top face, alone.
        "every_hole_taken_away" => (c(plate, TOP, None, false, false), Expect::Sheet(square, 1)),
        "the_hole_at_a_point_taken_away" => (c(plate, TOP, Some(&[RIM]), false, false), Expect::Sheet(square, 1)),
        // The outer boundary back on the plane's box of the face's span, the hole kept.
        "outer_boundary_back_hole_kept" => (c(plate, TOP, Some(&[]), true, false), Expect::Sheet(holed, 1)),
        "outer_boundary_back_holes_gone" => (c(plate, TOP, None, true, false), Expect::Sheet(square, 1)),
        // In the solid, the hole's edges are the bore's too: no hole of a sheet.
        "a_hole_of_a_solid_in_place" => (c(plate, TOP, None, false, true), Expect::Fails(Error::Unsupported)),
        "a_point_near_no_face" => (c(plate, [50.0, 50.0, 50.0], None, false, false), Expect::Fails(Error::NoMatch { point: 0 })),
        // The holed sheet, in place.
        "sheet_every_hole_taken_away" => (c(sheet, SHEET, None, false, true), Expect::Sheet(square, 1)),
        "sheet_the_hole_at_a_point_taken_away" => (c(sheet, SHEET, Some(&[SHEET_RIM]), false, true), Expect::Sheet(square, 1)),
        "sheet_outer_boundary_back_hole_kept" => (c(sheet, SHEET, Some(&[]), true, true), Expect::Sheet(holed, 1)),
        "sheet_outer_boundary_back_holes_gone" => (c(sheet, SHEET, None, true, true), Expect::Sheet(square, 1)),
        "sheet_a_point_near_no_hole" => (c(sheet, SHEET, Some(&[SHEET]), false, true), Expect::Fails(Error::NoMatch { point: 0 })),
        _ => panic!("no case {name}"),
    }
}

#[derive(Debug, PartialEq)]
struct Measured {
    area: f64,
    counts: [usize; 3],
}

type Got = std::result::Result<Measured, Error>;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

fn judge(name: &str, who: &str, got: Got) -> Got {
    let (_, expect) = case(name);
    match (&expect, &got) {
        (Expect::Fails(e), Err(g)) => assert_eq!(g, e, "{name}: {who} failed otherwise"),
        (Expect::Fails(e), Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
        (_, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (Expect::Sheet(a, faces), Ok(m)) => {
            assert!(rel(m.area, *a) < 1e-9, "{name}: {who} area {} vs analytic {a}", m.area);
            assert_eq!(m.counts[0], *faces, "{name}: {who} faces");
        }
    }
    got
}

fn solidcore_case(name: &str) -> Got {
    let (c, _) = case(name);
    let got = untrim_face(&c.input.core(), c.point, TOL, c.holes, c.outer, c.in_place).map(|b| {
        let (rec, _) = native(&to_arrays(&b).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
        rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
        Measured { area: area(&b, 1e-8).unwrap(), counts: counts(&b).unwrap() }
    });
    judge(name, "solidcore", got)
}

/// The case on the C++ kernel, called directly (whatever the routing), on the plate converted
/// from solidcore's.
#[cfg(feature = "native")]
fn cpp_case(name: &str) -> Got {
    use crate::{Brep, error, ffi};
    crate::init().unwrap();
    let (c, _) = case(name);
    let body = Brep::made_core(c.input.core());
    let r = ffi::trim::untrim_face(body.cpp().unwrap(), &c.point, TOL, c.holes, c.outer, c.in_place).map_err(|s| match s {
        ffi::ops::ERR_PICK_NONE => Error::NoMatch { point: 0 },
        s => error(s),
    });
    let got = r.map(|h| {
        let a = ffi::measure::area(&h, 1e-8).unwrap();
        let n = ffi::counts(&h).unwrap().map(|x| usize::try_from(x).unwrap());
        let (rec, _) = native(&Brep::made(h).cpp_arrays().unwrap()).unwrap();
        rec.validate().unwrap();
        Measured { area: a, counts: n }
    });
    judge(name, "C++", got)
}

#[cfg(feature = "native")]
fn parity(name: &str) {
    match (solidcore_case(name), cpp_case(name)) {
        (Ok(a), Ok(b)) => {
            assert!(rel(a.area, b.area) < 1e-9, "{name}: solidcore {} vs C++ {}", a.area, b.area);
            assert_eq!(a.counts, b.counts, "{name}: counts (solidcore vs C++)");
        }
        (a, b) => assert_eq!(a.err(), b.err(), "{name}: errors (solidcore vs C++)"),
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
                super::parity(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            fn the_cpp_kernel_matches_the_closed_form() {
                let _ = super::cpp_case(stringify!($m));
            }
        }
    )*};
}

cases!(
    every_hole_taken_away,
    the_hole_at_a_point_taken_away,
    outer_boundary_back_hole_kept,
    outer_boundary_back_holes_gone,
    a_hole_of_a_solid_in_place,
    a_point_near_no_face,
    sheet_every_hole_taken_away,
    sheet_the_hole_at_a_point_taken_away,
    sheet_outer_boundary_back_hole_kept,
    sheet_outer_boundary_back_holes_gone,
    sheet_a_point_near_no_hole,
);

/// The capi's argument checks, before any kernel call.
#[test]
fn hostile_arguments_are_refused() {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let p = [5.0, 5.0, 10.0];
    assert_eq!(untrim_face(&b, [f64::NAN, 0.0, 0.0], TOL, None, true, false).err(), Some(Error::Argument));
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(untrim_face(&b, p, t, None, true, false).err(), Some(Error::Argument), "tolerance {t}");
    }
    assert_eq!(untrim_face(&b, p, TOL, Some(&[]), false, false).err(), Some(Error::Argument), "nothing to do");
    assert_eq!(untrim_face(&b, p, TOL, Some(&[[f64::INFINITY; 3]]), false, false).err(), Some(Error::Argument));
    let many = vec![p; 10_001];
    assert_eq!(untrim_face(&b, p, TOL, Some(&many), false, false).err(), Some(Error::Argument));
}

/// The body untrimmed is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let b = plate();
    let was = counts(&b).unwrap();
    let _ = untrim_face(&b, TOP, TOL, None, true, false);
    let _ = untrim_face(&b, TOP, TOL, None, false, true);
    assert_eq!(counts(&b).unwrap(), was);
}

/// Face and edge tags (`Brep::tagged`) through an untrim routed to solidcore, as on C++.
#[cfg(feature = "native")]
mod history {
    use crate::{Brep, ffi};

    #[test]
    fn tags_cross_an_untrim() {
        crate::init().unwrap();
        let a = Brep::made_core(super::holed_sheet()).tagged(0).unwrap();
        for (holes, outer) in [(None, false), (Some(&[][..]), true)] {
            let cpp = ffi::trim::untrim_face(a.cpp().unwrap(), &super::SHEET, super::TOL, holes, outer, true).unwrap();
            let cpp = Brep::made(cpp).history().unwrap();
            let core = super::untrim_face(a.core().unwrap(), super::SHEET, super::TOL, holes, outer, true).unwrap();
            let core = Brep::made_core(core).history().unwrap();
            assert_eq!(core.canonical(), cpp.canonical(), "holes {holes:?}, outer {outer}");
        }
    }
}

/// A one-face sheet on `surface` over `domain`, the face its whole box.
fn whole_face(surface: solidcore::surface::surface::Surface) -> Body {
    use solidcore::topology::brep::p3::new_brep;
    use solidcore::topology::brep::p8::create_face_from_surface;
    super::run("sheet", |k| {
        let mut b = new_brep(k, solidcore::geom::tol::USE_DEFAULT, solidcore::geom::tol::USE_DEFAULT)?;
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        super::sweep::build_faces(&mut b, |b, fenv| create_face_from_surface(k, b, Some(surface), &domain, fenv))?;
        b.refresh_cached_topology();
        Ok(b)
    })
    .map(Body::new)
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Sheet {
    /// [0, 10]^2 at z = 0.
    Square,
    /// Radius 10 about +Z, from 0 to 90 degrees, z from 0 to 10.
    QuarterCylinder,
}

impl Sheet {
    fn core(self) -> Body {
        use solidcore::geom::extent2d::Extent2d;
        use solidcore::geom::vector2d::Vec2;
        use solidcore::geom::vector3d::Vec3;
        use solidcore::surface::cone::p1::Cone;
        use solidcore::surface::plane::p1::Plane;
        use solidcore::surface::surface::Surface;
        let (o, x, y) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        match self {
            Sheet::Square => whole_face(Surface::from(Plane::new(o, x, y, Vec2::new(1.0, 1.0), &Extent2d::from_bounds(0.0, 0.0, 10.0, 10.0)))),
            Sheet::QuarterCylinder => whole_face(Surface::from(Cone::new_cylinder_bounded(o, x, y, 10.0, 0.0, 90.0, 10.0, false, false, false))),
        }
    }
}

fn extend_case(name: &str) -> (Sheet, [f64; 3], f64, Continuation, Expect) {
    let quarter = 5.0 * PI;
    match name {
        "extend_a_square" => (Sheet::Square, [10.0, 5.0, 0.0], 5.0, Continuation::Natural, Expect::Sheet(150.0, 1)),
        "extend_a_square_tangent" => (Sheet::Square, [5.0, 10.0, 0.0], 5.0, Continuation::Tangent, Expect::Sheet(150.0, 1)),
        "extend_a_square_curvature" => (Sheet::Square, [0.0, 5.0, 0.0], 2.0, Continuation::Curvature, Expect::Sheet(120.0, 1)),
        "extend_a_cylinder_along_its_axis" => {
            (Sheet::QuarterCylinder, [FRAC_1_SQRT_2 * 10.0, FRAC_1_SQRT_2 * 10.0, 10.0], 5.0, Continuation::Natural, Expect::Sheet(quarter * 15.0, 1))
        }
        "extend_a_cylinder_round_its_axis" => {
            (Sheet::QuarterCylinder, [10.0, 0.0, 5.0], 5.0, Continuation::Natural, Expect::Sheet(10.0 * (quarter + 5.0), 1))
        }
        // Run-offs 5 long across the surface whatever their curvature: the strip stays its section
        // swept 10 along z. A circle's G2 run-off runs about 2.3 times what it is asked and more
        // than doubles with it (fork 0f7b209), so the ask must be solved for.
        "extend_a_cylinder_tangent_run_off" => {
            (Sheet::QuarterCylinder, [10.0, 0.0, 5.0], 5.0, Continuation::Tangent, Expect::Sheet(10.0 * (quarter + 5.0), 1))
        }
        "extend_a_cylinder_curvature_run_off" => {
            (Sheet::QuarterCylinder, [10.0, 0.0, 5.0], 5.0, Continuation::Curvature, Expect::Sheet(10.0 * (quarter + 5.0), 1))
        }
        "extend_picks_near_no_edge" => (Sheet::Square, [50.0, 50.0, 0.0], 5.0, Continuation::Natural, Expect::Fails(Error::NoMatch { point: 0 })),
        _ => panic!("no case {name}"),
    }
}

fn extend_judge(name: &str, who: &str, got: Got) -> Got {
    let (.., expect) = extend_case(name);
    match (&expect, &got) {
        (Expect::Fails(e), Err(g)) => assert_eq!(g, e, "{name}: {who} failed otherwise"),
        (Expect::Fails(e), Ok(m)) => panic!("{name}: {who} gave {m:?}, expected {e:?}"),
        (_, Err(g)) => panic!("{name}: {who} failed: {g}"),
        (Expect::Sheet(a, faces), Ok(m)) => {
            assert!(rel(m.area, *a) < 1e-6, "{name}: {who} area {} vs analytic {a}", m.area);
            assert_eq!(m.counts[0], *faces, "{name}: {who} faces");
        }
    }
    got
}

fn extend_solidcore(name: &str) -> Got {
    let (sheet, point, distance, how, _) = extend_case(name);
    let got = extend_sheet(&sheet.core(), point, TOL, distance, how).map(|b| {
        let (rec, _) = native(&to_arrays(&b).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
        rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
        Measured { area: area(&b, 1e-8).unwrap(), counts: counts(&b).unwrap() }
    });
    extend_judge(name, "solidcore", got)
}

#[cfg(feature = "native")]
fn extend_cpp(name: &str) -> Got {
    use crate::{Brep, error, ffi};
    crate::init().unwrap();
    let (sheet, point, distance, how, _) = extend_case(name);
    let mode = match how {
        Continuation::Natural => ffi::trim::EXTEND_NATURAL,
        Continuation::Tangent => ffi::trim::EXTEND_TANGENT,
        Continuation::Curvature => ffi::trim::EXTEND_CURVATURE,
    };
    let body = Brep::made_core(sheet.core());
    let r = ffi::trim::extend_sheet(body.cpp().unwrap(), &point, TOL, distance, mode).map_err(|s| match s {
        ffi::ops::ERR_PICK_NONE => Error::NoMatch { point: 0 },
        s => error(s),
    });
    let got = r.map(|h| {
        let a = ffi::measure::area(&h, 1e-8).unwrap();
        let n = ffi::counts(&h).unwrap().map(|x| usize::try_from(x).unwrap());
        let (rec, _) = native(&Brep::made(h).cpp_arrays().unwrap()).unwrap();
        rec.validate().unwrap();
        Measured { area: a, counts: n }
    });
    extend_judge(name, "C++", got)
}

#[cfg(feature = "native")]
fn extend_parity(name: &str) {
    match (extend_solidcore(name), extend_cpp(name)) {
        (Ok(a), Ok(b)) => {
            assert!(rel(a.area, b.area) < 1e-9, "{name}: solidcore {} vs C++ {}", a.area, b.area);
            assert_eq!(a.counts, b.counts, "{name}: counts (solidcore vs C++)");
        }
        (a, b) => assert_eq!(a.err(), b.err(), "{name}: errors (solidcore vs C++)"),
    }
}

macro_rules! extend_cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_closed_form() {
                let _ = super::extend_solidcore(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_cpp_kernel() {
                super::extend_parity(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            fn the_cpp_kernel_matches_the_closed_form() {
                let _ = super::extend_cpp(stringify!($m));
            }
        }
    )*};
}

extend_cases!(
    extend_a_square,
    extend_a_square_tangent,
    extend_a_square_curvature,
    extend_a_cylinder_along_its_axis,
    extend_a_cylinder_round_its_axis,
    extend_a_cylinder_tangent_run_off,
    extend_a_cylinder_curvature_run_off,
    extend_picks_near_no_edge,
);

/// The capi's argument checks for an extend, before any kernel call, and a body of more than
/// one face.
#[test]
fn hostile_extend_arguments_are_refused() {
    let s = Sheet::Square.core();
    let p = [10.0, 5.0, 0.0];
    assert_eq!(extend_sheet(&s, [f64::NAN; 3], TOL, 5.0, Continuation::Natural).err(), Some(Error::Argument));
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(extend_sheet(&s, p, t, 5.0, Continuation::Natural).err(), Some(Error::Argument), "tolerance {t}");
        assert_eq!(extend_sheet(&s, p, TOL, t, Continuation::Natural).err(), Some(Error::Argument), "distance {t}");
    }
    let cube = make_box([0.0; 3], [10.0; 3]).unwrap();
    assert_eq!(extend_sheet(&cube, p, TOL, 5.0, Continuation::Natural).err(), Some(Error::Unsupported));
    // A face that is not its surface's whole box: a square with a hole.
    assert_eq!(extend_sheet(&holed_sheet(), [20.0, 5.0, 0.0], TOL, 5.0, Continuation::Natural).err(), Some(Error::Unsupported));
}
