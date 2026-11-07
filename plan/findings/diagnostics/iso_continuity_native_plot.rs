//! Export native constant-U geometry and its currently observed metadata.
use solidcore::{
    curve::{curve::Curve, iso_curve::IsoCurve},
    nurbs::{
        b_spline_surface::{BSplineSurface, SurfParamType},
        math::CPoint,
        srf_basic::Surface as Net,
    },
    Kernel,
};
fn main() {
    let k = Kernel::default();
    let rows = [0., 1.]
        .into_iter()
        .map(|z| {
            vec![
                CPoint::new(-0.25, 0., z, 1.),
                CPoint::new(0., 1., z, 1.),
                CPoint::new(0.25, 0., z, 1.),
            ]
        })
        .collect();
    let s = BSplineSurface::from_nurb(
        Net::new(rows, 1, 1, vec![0., 0., 1., 1.], vec![0., 0., 0.5, 2., 2.]).unwrap(),
        false,
    );
    let c = Curve::from(IsoCurve::new(s, SurfParamType::U, 0.5, false, true));
    let (knots, _) = c.knots(&k, None).unwrap();
    let (_, conts) = c.calculate_continuities(&k, 0.5).unwrap();
    println!(
        "{{\"observed_continuity_count\":{},\"native_points\":[",
        conts.len()
    );
    for (j, t) in knots.into_iter().enumerate() {
        let p = c.evaluate_point(&k, t).unwrap();
        if j > 0 {
            println!(",");
        }
        print!("[{t},{},{},{}]", p.x, p.y, p.z);
    }
    println!("]}}");
}
