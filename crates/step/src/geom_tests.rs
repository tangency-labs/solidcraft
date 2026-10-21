//! STEP geometry, one entity at a time: reading (values, units, evaluation in STEP's own
//! parameterisation), writing, and read → write → read round trips. Expected values are
//! analytic.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
use std::sync::Arc;

use crate::geom::{Associated, Curve, CurveForm, KnotSpec, Logical, P3, PreferredRep, Surface, SurfaceCurveKind, Trim};
use crate::p21::{self, Exchange};
use crate::read::{Reader, Units};
use crate::write::{GeomWriter, Writer, real};

const MM: Units = Units { len: 1.0, ang: 1.0 };

fn file(data: &str) -> Exchange {
    let t = format!("ISO-10303-21;\nHEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n");
    match p21::parse(&t) {
        Ok(ex) => ex,
        Err(e) => panic!("{e}\n{t}"),
    }
}

/// Common entities: #1 origin, #2 +Z, #3 +X, #4 the world placement, #5 +Y.
const BASE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));#3=DIRECTION('',(1.,0.,0.));\
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);#5=DIRECTION('',(0.,1.,0.));";

fn curve(data: &str, id: u64, units: Units) -> Arc<Curve> {
    let ex = file(&format!("{BASE}{data}"));
    match Reader::new(&ex, units).curve(id) {
        Ok(c) => c,
        Err(e) => panic!("{e}"),
    }
}

fn surface(data: &str, id: u64, units: Units) -> Arc<Surface> {
    let ex = file(&format!("{BASE}{data}"));
    match Reader::new(&ex, units).surface(id) {
        Ok(s) => s,
        Err(e) => panic!("{e}"),
    }
}

fn close(a: P3, b: P3) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9 * (1.0 + y.abs()))
}

#[track_caller]
fn at_curve(c: &Curve, t: f64, want: P3) {
    let got = c.point(t);
    assert!(got.is_some_and(|g| close(g, want)), "at {t}: got {got:?}, want {want:?}");
}

#[track_caller]
fn at_surface(s: &Surface, u: f64, v: f64, want: P3) {
    let got = s.point(u, v);
    assert!(got.is_some_and(|g| close(g, want)), "at ({u}, {v}): got {got:?}, want {want:?}");
}

/// Writes a curve or surface, reads it back, writes it again: the second file must equal the
/// first, and the geometry read back must evaluate like the original.
fn round_trip_curve(c: &Curve, params: &[f64]) {
    let mut w = Writer::new();
    let id = GeomWriter::new(&mut w).curve(c);
    let text = w.file("t", "AUTOMOTIVE_DESIGN");
    let ex = p21::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let back = Reader::new(&ex, MM).curve(id).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(std::mem::discriminant(c), std::mem::discriminant(&*back), "{text}");
    for &t in params {
        let (a, b) = (c.point(t), back.point(t));
        assert!(matches!((a, b), (Some(a), Some(b)) if close(a, b)) || (a.is_none() && b.is_none()), "at {t}: {a:?} vs {b:?}\n{text}");
    }
    let mut w2 = Writer::new();
    GeomWriter::new(&mut w2).curve(&back);
    assert_eq!(w2.data(), w.data(), "a second write differs");
}

fn round_trip_surface(s: &Surface, params: &[(f64, f64)]) {
    let mut w = Writer::new();
    let id = GeomWriter::new(&mut w).surface(s);
    let text = w.file("t", "AUTOMOTIVE_DESIGN");
    let ex = p21::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let back = Reader::new(&ex, MM).surface(id).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(std::mem::discriminant(s), std::mem::discriminant(&*back), "{text}");
    for &(u, v) in params {
        let (a, b) = (s.point(u, v), back.point(u, v));
        assert!(matches!((a, b), (Some(a), Some(b)) if close(a, b)), "at ({u}, {v}): {a:?} vs {b:?}\n{text}");
    }
    let mut w2 = Writer::new();
    GeomWriter::new(&mut w2).surface(&back);
    assert_eq!(w2.data(), w.data(), "a second write differs");
}

const TS: [f64; 5] = [0.0, 0.3, 1.0, FRAC_PI_2, 2.5];
const UVS: [(f64, f64); 4] = [(0.0, 0.0), (0.4, 0.2), (FRAC_PI_2, 1.0), (2.0, -0.5)];

#[test]
fn line() {
    let c = curve("#10=CARTESIAN_POINT('',(1.,2.,3.));#11=VECTOR('',#3,5.);#12=LINE('',#10,#11);", 12, MM);
    assert!(matches!(*c, Curve::Line { magnitude, .. } if magnitude == 5.0));
    at_curve(&c, 2.0, [11.0, 2.0, 3.0]);
    round_trip_curve(&c, &TS);
}

#[test]
fn circle_and_ellipse() {
    let c = curve("#10=CIRCLE('',#4,10.);", 10, MM);
    at_curve(&c, 0.0, [10.0, 0.0, 0.0]);
    at_curve(&c, FRAC_PI_2, [0.0, 10.0, 0.0]);
    round_trip_curve(&c, &TS);
    let e = curve("#10=ELLIPSE('',#4,10.,4.);", 10, MM);
    at_curve(&e, FRAC_PI_2, [0.0, 4.0, 0.0]);
    at_curve(&e, PI, [-10.0, 0.0, 0.0]);
    round_trip_curve(&e, &TS);
}

#[test]
fn hyperbola_and_parabola() {
    let h = curve("#10=HYPERBOLA('',#4,2.,3.);", 10, MM);
    at_curve(&h, 1.0, [2.0 * 1f64.cosh(), 3.0 * 1f64.sinh(), 0.0]);
    round_trip_curve(&h, &TS);
    let p = curve("#10=PARABOLA('',#4,0.5);", 10, MM);
    at_curve(&p, 2.0, [2.0, 2.0, 0.0]);
    round_trip_curve(&p, &TS);
}

#[test]
fn bspline_curve_with_knots() {
    // A quadratic Bézier from (0,0) through control (1,2) to (2,0): its middle is (1, 1).
    let c = curve(
        "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(1.,2.,0.));#12=CARTESIAN_POINT('',(2.,0.,0.));\
         #13=B_SPLINE_CURVE_WITH_KNOTS('',2,(#10,#11,#12),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.PIECEWISE_BEZIER_KNOTS.);",
        13,
        MM,
    );
    let Curve::BSpline(b) = &*c else { panic!("{c:?}") };
    assert_eq!((b.degree, b.multiplicities.clone(), b.knot_spec), (2, vec![3, 3], KnotSpec::PiecewiseBezierKnots));
    assert_eq!(b.domain(), Some((0.0, 1.0)));
    at_curve(&c, 0.5, [1.0, 1.0, 0.0]);
    round_trip_curve(&c, &[0.0, 0.25, 0.5, 1.0]);
}

#[test]
fn rational_bspline_curve_is_an_exact_quarter_circle() {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let c = curve(
        &format!(
            "#10=CARTESIAN_POINT('',(1.,0.,0.));#11=CARTESIAN_POINT('',(1.,1.,0.));#12=CARTESIAN_POINT('',(0.,1.,0.));\
             #13=(BOUNDED_CURVE()B_SPLINE_CURVE(2,(#10,#11,#12),.CIRCULAR_ARC.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS((3,3),(0.,1.),.UNSPECIFIED.)\
             CURVE()GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE((1.,{},1.))REPRESENTATION_ITEM(''));",
            real(w)
        ),
        13,
        MM,
    );
    let Curve::BSpline(b) = &*c else { panic!("{c:?}") };
    assert_eq!(b.form, CurveForm::CircularArc);
    assert!(b.weights.is_some());
    for t in [0.1, 0.5, 0.9] {
        let p = c.point(t).unwrap_or_default();
        assert!((p[0].hypot(p[1]) - 1.0).abs() < 1e-12, "{p:?}");
    }
    round_trip_curve(&c, &[0.0, 0.3, 0.7, 1.0]);
}

#[test]
fn implied_knots_and_repeated_knots() {
    let pts = "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(1.,1.,0.));#12=CARTESIAN_POINT('',(2.,0.,0.));#13=CARTESIAN_POINT('',(3.,1.,0.));";
    let q = curve(&format!("{pts}#14=QUASI_UNIFORM_CURVE('',2,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.);"), 14, MM);
    let Curve::BSpline(b) = &*q else { panic!("{q:?}") };
    assert_eq!((b.knots.clone(), b.multiplicities.clone()), (vec![0.0, 1.0, 2.0], vec![3, 1, 3]));
    at_curve(&q, 0.0, [0.0, 0.0, 0.0]);
    at_curve(&q, 2.0, [3.0, 1.0, 0.0]);
    let bez = curve(&format!("{pts}#14=BEZIER_CURVE('',3,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.);"), 14, MM);
    let Curve::BSpline(b) = &*bez else { panic!("{bez:?}") };
    assert_eq!((b.knots.clone(), b.multiplicities.clone()), (vec![0.0, 1.0], vec![4, 4]));
    // A knot written twice is one knot.
    let rep = curve(
        &format!("{pts}#14=B_SPLINE_CURVE_WITH_KNOTS('',2,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.,(3,1,1,2),(0.,1.,1.,2.),.UNSPECIFIED.);"),
        14,
        MM,
    );
    let Curve::BSpline(b) = &*rep else { panic!("{rep:?}") };
    assert_eq!((b.knots.clone(), b.multiplicities.clone()), (vec![0.0, 1.0, 2.0], vec![3, 2, 2]));
    assert_eq!(b.domain(), Some((0.0, 1.0)));
    round_trip_curve(&rep, &[0.0, 0.5, 1.0]);
}

#[test]
fn polyline_and_trimmed_curve() {
    let pl = curve(
        "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(4.,0.,0.));#12=CARTESIAN_POINT('',(4.,2.,0.));#13=POLYLINE('',(#10,#11,#12));",
        13,
        MM,
    );
    at_curve(&pl, 0.5, [2.0, 0.0, 0.0]);
    at_curve(&pl, 1.5, [4.0, 1.0, 0.0]);
    round_trip_curve(&pl, &[0.0, 0.5, 1.0, 1.5, 2.0]);
    // A circle trimmed in degrees: the parameters become radians.
    let deg = Units { len: 1.0, ang: PI / 180.0 };
    let t = curve(
        "#10=CIRCLE('',#4,10.);#11=CARTESIAN_POINT('',(10.,0.,0.));#12=TRIMMED_CURVE('',#10,(#11,PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);",
        12,
        deg,
    );
    let Curve::Trimmed { trim_1, trim_2, sense, .. } = &*t else { panic!("{t:?}") };
    assert_eq!(trim_1.first(), Some(&Trim::Point([10.0, 0.0, 0.0])));
    assert!(matches!(trim_2.first(), Some(Trim::Parameter(a)) if (a - FRAC_PI_2).abs() < 1e-15));
    assert!(*sense);
    round_trip_curve(&t, &TS);
}

#[test]
fn surface_curve_with_pcurves_shares_its_surface() {
    // A seam on a cylinder of radius 5: the line x = 5 with pcurves u = 0 on both sides.
    let c = curve(
        "#10=CYLINDRICAL_SURFACE('',#4,5.);#11=CARTESIAN_POINT('',(5.,0.,0.));#12=VECTOR('',#2,1.);#13=LINE('',#11,#12);\
         #20=CARTESIAN_POINT('',(0.,0.));#21=DIRECTION('',(0.,1.));#22=VECTOR('',#21,1.);#23=LINE('',#20,#22);\
         #24=(GEOMETRIC_REPRESENTATION_CONTEXT(2)PARAMETRIC_REPRESENTATION_CONTEXT()REPRESENTATION_CONTEXT('2D SPACE',''));\
         #25=DEFINITIONAL_REPRESENTATION('',(#23),#24);#26=PCURVE('',#10,#25);\
         #27=CARTESIAN_POINT('',(6.28318530717959,0.));#28=LINE('',#27,#22);#29=DEFINITIONAL_REPRESENTATION('',(#28),#24);#30=PCURVE('',#10,#29);\
         #31=SEAM_CURVE('',#13,(#26,#30),.PCURVE_S1.);",
        31,
        MM,
    );
    let Curve::SurfaceCurve { kind, associated, master, curve_3d } = &*c else { panic!("{c:?}") };
    assert_eq!((*kind, *master), (SurfaceCurveKind::Seam, PreferredRep::PcurveS1));
    at_curve(curve_3d, 3.0, [5.0, 0.0, 3.0]);
    let [Associated::Pcurve(a), Associated::Pcurve(b)] = associated.as_slice() else { panic!("{associated:?}") };
    assert!(Arc::ptr_eq(&a.surface, &b.surface), "one surface entity, read once");
    // Each pcurve, lifted onto the surface, is the 3D curve.
    for t in [0.0, 2.0] {
        for pc in [a, b] {
            let uv = pc.curve.point(t).unwrap_or_default();
            let p = pc.surface.point(uv[0], uv[1]).unwrap_or_default();
            assert!(close(p, curve_3d.point(t).unwrap_or_default()), "{p:?}");
        }
    }
    round_trip_curve(&c, &[0.0, 1.0, 4.0]);
}

#[test]
fn pcurves_scale_with_the_surface_parameters() {
    // Inches and degrees: a cylinder pcurve's u is an angle, v a length.
    let units = Units { len: 25.4, ang: PI / 180.0 };
    let c = curve(
        "#10=CYLINDRICAL_SURFACE('',#4,1.);#11=CARTESIAN_POINT('',(1.,0.,0.));#12=VECTOR('',#2,1.);#13=LINE('',#11,#12);\
         #20=CARTESIAN_POINT('',(90.,0.));#21=DIRECTION('',(0.,1.));#22=VECTOR('',#21,1.);#23=LINE('',#20,#22);\
         #24=(GEOMETRIC_REPRESENTATION_CONTEXT(2)PARAMETRIC_REPRESENTATION_CONTEXT()REPRESENTATION_CONTEXT('2D SPACE',''));\
         #25=DEFINITIONAL_REPRESENTATION('',(#23),#24);#26=PCURVE('',#10,#25);#27=SURFACE_CURVE('',#13,(#26),.CURVE_3D.);",
        27,
        units,
    );
    let Curve::SurfaceCurve { associated, .. } = &*c else { panic!("{c:?}") };
    let Some(Associated::Pcurve(pc)) = associated.first() else { panic!("{associated:?}") };
    let uv = pc.curve.point(2.0).unwrap_or_default();
    assert!((uv[0] - FRAC_PI_2).abs() < 1e-12 && (uv[1] - 2.0 * 25.4).abs() < 1e-9, "{uv:?}");
    at_surface(&pc.surface, uv[0], uv[1], [0.0, 25.4, 50.8]);
}

#[test]
fn analytic_surfaces() {
    let p = surface("#10=PLANE('',#4);", 10, MM);
    at_surface(&p, 2.0, 3.0, [2.0, 3.0, 0.0]);
    round_trip_surface(&p, &UVS);
    let c = surface("#10=CYLINDRICAL_SURFACE('',#4,5.);", 10, MM);
    at_surface(&c, FRAC_PI_2, 7.0, [0.0, 5.0, 7.0]);
    round_trip_surface(&c, &UVS);
    let k = surface("#10=CONICAL_SURFACE('',#4,5.,0.785398163397448);", 10, MM);
    at_surface(&k, 0.0, 2.0, [7.0, 0.0, 2.0]);
    round_trip_surface(&k, &UVS);
    let s = surface("#10=SPHERICAL_SURFACE('',#4,3.);", 10, MM);
    at_surface(&s, 0.0, FRAC_PI_2, [0.0, 0.0, 3.0]);
    at_surface(&s, PI, 0.0, [-3.0, 0.0, 0.0]);
    round_trip_surface(&s, &UVS);
    let t = surface("#10=TOROIDAL_SURFACE('',#4,10.,2.);", 10, MM);
    at_surface(&t, FRAC_PI_2, PI, [0.0, 8.0, 0.0]);
    round_trip_surface(&t, &UVS);
    let d = surface("#10=DEGENERATE_TOROIDAL_SURFACE('',#4,1.,2.,.T.);", 10, MM);
    assert!(matches!(*d, Surface::DegenerateTorus { select_outer: true, .. }));
    round_trip_surface(&d, &UVS);
}

#[test]
fn units_scale_lengths_and_angles() {
    let units = Units { len: 25.4, ang: PI / 180.0 };
    let k = surface("#10=CONICAL_SURFACE('',#4,1.,45.);", 10, units);
    let Surface::Cone { radius, semi_angle, .. } = *k else { panic!("{k:?}") };
    assert!((radius - 25.4).abs() < 1e-12 && (semi_angle - FRAC_PI_4).abs() < 1e-15);
    let c = curve("#10=CARTESIAN_POINT('',(1.,0.,0.));#11=VECTOR('',#3,2.);#12=LINE('',#10,#11);", 12, units);
    at_curve(&c, 1.0, [76.2, 0.0, 0.0]);
}

#[test]
fn placement_axes_are_made_orthonormal() {
    // A reference direction not perpendicular to the axis is projected; the axis is unit.
    let s = surface("#10=DIRECTION('',(0.,0.,2.));#11=DIRECTION('',(1.,0.,1.));#12=AXIS2_PLACEMENT_3D('',#1,#10,#11);#13=PLANE('',#12);", 13, MM);
    let Surface::Plane { position } = *s else { panic!("{s:?}") };
    assert_eq!((position.axis, position.ref_direction), ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
}

#[test]
fn bspline_surfaces() {
    // A bilinear patch over the unit square, then the same as a rational complex entity.
    let pts = "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(0.,1.,0.));#12=CARTESIAN_POINT('',(1.,0.,0.));#13=CARTESIAN_POINT('',(1.,1.,1.));";
    let s = surface(
        &format!(
            "{pts}#14=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#10,#11),(#12,#13)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);"
        ),
        14,
        MM,
    );
    at_surface(&s, 0.5, 0.5, [0.5, 0.5, 0.25]);
    at_surface(&s, 1.0, 0.0, [1.0, 0.0, 0.0]);
    round_trip_surface(&s, &[(0.0, 0.0), (0.3, 0.7), (1.0, 1.0)]);
    let r = surface(
        &format!(
            "{pts}#14=(BOUNDED_SURFACE()B_SPLINE_SURFACE(1,1,((#10,#11),(#12,#13)),.UNSPECIFIED.,.F.,.F.,.F.)\
             B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)GEOMETRIC_REPRESENTATION_ITEM()\
             RATIONAL_B_SPLINE_SURFACE(((1.,1.),(1.,1.)))REPRESENTATION_ITEM('')SURFACE());"
        ),
        14,
        MM,
    );
    let Surface::BSpline(b) = &*r else { panic!("{r:?}") };
    assert!(b.weights.is_some());
    at_surface(&r, 0.5, 0.5, [0.5, 0.5, 0.25]);
    round_trip_surface(&r, &[(0.0, 0.0), (0.3, 0.7), (1.0, 1.0)]);
}

#[test]
fn swept_surfaces() {
    // A circle of radius 2 about the Z axis, centred at x = 10: revolved, a torus.
    let rev = surface(
        "#10=CARTESIAN_POINT('',(10.,0.,0.));#11=AXIS2_PLACEMENT_3D('',#10,#5,#3);#12=CIRCLE('',#11,2.);\
         #13=AXIS1_PLACEMENT('',#1,#2);#14=SURFACE_OF_REVOLUTION('',#12,#13);",
        14,
        MM,
    );
    at_surface(&rev, 0.0, 0.0, [12.0, 0.0, 0.0]);
    at_surface(&rev, FRAC_PI_2, 0.0, [0.0, 12.0, 0.0]);
    round_trip_surface(&rev, &UVS);
    let ext = surface("#10=CIRCLE('',#4,3.);#11=VECTOR('',#2,4.);#12=SURFACE_OF_LINEAR_EXTRUSION('',#10,#11);", 12, MM);
    at_surface(&ext, FRAC_PI_2, 0.5, [0.0, 3.0, 2.0]);
    round_trip_surface(&ext, &UVS);
}

#[test]
fn offset_and_trimmed_surfaces() {
    let o = surface("#10=CYLINDRICAL_SURFACE('',#4,5.);#11=OFFSET_SURFACE('',#10,1.5,.F.);", 11, MM);
    at_surface(&o, 0.0, 1.0, [6.5, 0.0, 1.0]);
    round_trip_surface(&o, &UVS);
    let deg = Units { len: 1.0, ang: PI / 180.0 };
    let t = surface("#10=CYLINDRICAL_SURFACE('',#4,5.);#11=RECTANGULAR_TRIMMED_SURFACE('',#10,0.,90.,0.,10.,.T.,.T.);", 11, deg);
    let Surface::RectangularTrimmed { u2, v2, .. } = *t else { panic!("{t:?}") };
    assert!((u2 - FRAC_PI_2).abs() < 1e-15 && v2 == 10.0, "{u2} {v2}");
    round_trip_surface(&t, &UVS);
}

#[test]
fn offset_curve_reads_and_round_trips() {
    let c = curve("#10=CIRCLE('',#4,10.);#11=OFFSET_CURVE_3D('',#10,1.,.F.,#2);", 11, MM);
    assert!(matches!(*c, Curve::OffsetCurve3d { distance, self_intersect: Logical::False, .. } if distance == 1.0));
    round_trip_curve(&c, &[]);
}

#[test]
fn shared_entities_are_written_once() {
    let c = Arc::new(Curve::Circle {
        position: crate::geom::Placement { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_direction: [1.0, 0.0, 0.0] },
        radius: 1.0,
    });
    let mut w = Writer::new();
    let mut g = GeomWriter::new(&mut w);
    let a = g.curve(&c);
    let b = g.curve(&c);
    assert_eq!(a, b);
    // Point, two directions, placement, circle.
    assert_eq!(w.len(), 5);
}

#[test]
fn hostile_geometry_is_an_error_never_a_panic() {
    let cases = [
        "#10=CIRCLE('',#4,-1.);",
        "#10=CIRCLE('',#99,1.);",
        "#10=CIRCLE('',#4,1.E300);",
        "#10=LINE('',#1,#1);",
        "#10=DIRECTION('',(0.,0.,0.));#11=VECTOR('',#10,1.);#12=LINE('',#1,#11);",
        "#10=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#1),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);",
        "#10=B_SPLINE_CURVE_WITH_KNOTS('',99,(#1,#1),.UNSPECIFIED.,.F.,.F.,(1,1),(0.,1.),.UNSPECIFIED.);",
        "#10=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#1),.UNSPECIFIED.,.F.,.F.,(4000000000000,1),(0.,1.),.UNSPECIFIED.);",
        "#10=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#1),.UNSPECIFIED.,.F.,.F.,(2,2),(1.,0.),.UNSPECIFIED.);",
        "#10=(B_SPLINE_CURVE(1,(#1,#1),.UNSPECIFIED.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS((2,2),(0.,1.),.UNSPECIFIED.)RATIONAL_B_SPLINE_CURVE((1.,-1.)));",
        "#10=TRIMMED_CURVE('',#10,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);",
        "#10=SURFACE_CURVE('',#10,(),.CURVE_3D.);",
        "#10=POLYLINE('',(#1));",
        "#10=UNKNOWN_CURVE('');",
    ];
    for c in cases {
        let ex = file(&format!("{BASE}{c}"));
        assert!(Reader::new(&ex, MM).curve(10).is_err(), "{c}");
    }
    let surfaces = [
        "#10=CYLINDRICAL_SURFACE('',#4,0.);",
        "#10=CONICAL_SURFACE('',#4,1.,2.);",
        "#10=TOROIDAL_SURFACE('',#4,1.,-2.);",
        "#10=OFFSET_SURFACE('',#10,1.,.F.);",
        "#10=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#1,#1),(#1)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
        "#10=SURFACE_OF_REVOLUTION('',#10,#4);",
        "#10=PLANE('',#3);",
    ];
    for s in surfaces {
        let ex = file(&format!("{BASE}{s}"));
        assert!(Reader::new(&ex, MM).surface(10).is_err(), "{s}");
    }
}

/// Every edge curve and face surface of every STEP file in `SOLIDCRAFT_STEP_CORPUS` (the
/// tangency-labs/models files): read, written, read back. Run with `--ignored`.
#[test]
#[ignore = "needs SOLIDCRAFT_STEP_CORPUS"]
fn corpus_geometry_reads_and_round_trips() {
    let Ok(dir) = std::env::var("SOLIDCRAFT_STEP_CORPUS") else { return };
    let mut paths: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    paths.sort();
    let mut failed = 0;
    for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))) {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let t0 = std::time::Instant::now();
        let ex = p21::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut reader = Reader::new(&ex, MM);
        let (mut curves, mut surfaces) = (Vec::new(), Vec::new());
        let mut errors: std::collections::BTreeMap<String, usize> = Default::default();
        let mut ids: Vec<u64> = ex.entities.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(e) = ex.get(id) else { continue };
            let (r, is_curve) = match e.name() {
                "EDGE_CURVE" => (e.params().get(3).and_then(p21::Param::as_ref_id), true),
                "ADVANCED_FACE" | "FACE_SURFACE" => (e.params().get(2).and_then(p21::Param::as_ref_id), false),
                _ => continue,
            };
            let Some(r) = r else { continue };
            let got = if is_curve { reader.curve(r).map(|c| curves.push(c)) } else { reader.surface(r).map(|s| surfaces.push(s)) };
            if let Err(e) = got {
                *errors.entry(e).or_default() += 1;
            }
        }
        let read = t0.elapsed();
        let mut w = Writer::new();
        let mut g = GeomWriter::new(&mut w);
        let cids: Vec<u64> = curves.iter().map(|c| g.curve(c)).collect();
        let sids: Vec<u64> = surfaces.iter().map(|s| g.surface(s)).collect();
        let out = w.file("corpus", "AUTOMOTIVE_DESIGN");
        let back = p21::parse(&out).unwrap_or_else(|e| panic!("{}: written file: {e}", path.display()));
        let mut again = Reader::new(&back, MM);
        let mut mismatched = 0;
        for (c, id) in curves.iter().zip(&cids) {
            match again.curve(*id) {
                Ok(b)
                    if (0..=4).map(|i| i as f64 * 0.25).all(|t| match (c.point(t), b.point(t)) {
                        (Some(x), Some(y)) => close(x, y),
                        (x, y) => x.is_none() && y.is_none(),
                    }) => {}
                _ => mismatched += 1,
            }
        }
        for (s, id) in surfaces.iter().zip(&sids) {
            match again.surface(*id) {
                Ok(b)
                    if [(0.0, 0.0), (0.3, 0.6)].iter().all(|&(u, v)| match (s.point(u, v), b.point(u, v)) {
                        (Some(x), Some(y)) => close(x, y),
                        (x, y) => x.is_none() && y.is_none(),
                    }) => {}
                _ => mismatched += 1,
            }
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!(
            "{name}: {} edge curves, {} face surfaces read in {:.0} ms; {} unread; {mismatched} differ after a round trip",
            curves.len(),
            surfaces.len(),
            read.as_secs_f64() * 1e3,
            errors.values().sum::<usize>()
        );
        for (e, n) in &errors {
            println!("    {n} × {e}");
        }
        failed += mismatched + errors.values().sum::<usize>();
    }
    assert_eq!(failed, 0);
}
