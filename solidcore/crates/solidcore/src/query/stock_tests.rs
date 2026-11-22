use super::*;
use crate::primitive::api_primitives::create_box;
use crate::{KernelOptions, Point3};

#[test]
fn a_prism_uses_its_faces_after_an_oblique_rigid_move() {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 600.0, 70.0, 35.0).unwrap();
    let q = 0.5_f64.sqrt();
    transform_matrix(
        &mut k,
        &mut b,
        &[
            [q, q, 0.0, 0.0],
            [-q, q, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [100.0, -200.0, 30.0, 1.0],
        ],
        true,
    )
    .unwrap();
    let stock = stock_box(&mut k, &mut b).unwrap();
    assert!(stock.exact);
    for (a, b) in stock.dimensions.iter().zip([600.0, 70.0, 35.0]) {
        assert!((a - b).abs() < 1e-7, "{stock:?}");
    }
    for axis in stock.axes {
        assert!((axis.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-12);
    }
}
