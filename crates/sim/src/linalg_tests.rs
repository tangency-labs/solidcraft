use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;

use crate::SimError;
use crate::grid::{Grid3, Occupancy};
use crate::linalg::Stencil7;
use crate::test_shapes::block_at;

/// A small cube of solid: 4 by 4 by 4 cells, 64 equations.
fn cube(span: usize) -> Occupancy {
    let size = 20.0;
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(size, size, size) };
    let grid = Grid3::over(b, span, 1).expect("lattice");
    let mesh = block_at([0., 0., 0.], [size, size, size]);
    Occupancy::of(grid, &[&mesh], 1).expect("occupancy")
}

/// The operator written out in full, for comparing against.
fn dense(a: &Stencil7) -> Vec<Vec<f64>> {
    let n = a.equations();
    let mut m = vec![vec![0.0; n]; n];
    for (e, row) in m.iter_mut().enumerate() {
        if let Some(slot) = row.get_mut(e) {
            *slot = a.diagonal(e);
        }
    }
    for axis in 0..3 {
        for e in 0..n {
            if let Some(to) = a.forward_of(e, axis) {
                let c = a.coupling(e, axis);
                if let Some(v) = m.get_mut(e).and_then(|r| r.get_mut(to)) {
                    *v += c;
                }
                if let Some(v) = m.get_mut(to).and_then(|r| r.get_mut(e)) {
                    *v += c;
                }
            }
        }
    }
    m
}

fn laplacian(occ: &Occupancy, conductance: f64) -> Stencil7 {
    let mut a = Stencil7::over(occ);
    for e in 0..occ.equations() {
        for axis in 0..3 {
            a.couple(e, axis, conductance);
        }
    }
    a
}

#[test]
fn the_operator_cannot_be_asymmetric_because_one_number_serves_both_ends() {
    let occ = cube(4);
    let mut a = Stencil7::over(&occ);
    // Deliberately uneven couplings, to catch an implementation that stored them twice.
    for e in 0..occ.equations() {
        for axis in 0..3 {
            a.couple(e, axis, 1.0 + e as f64 * 0.25 + axis as f64);
        }
        a.add_diagonal(e, 0.5);
    }
    let m = dense(&a);
    for (i, row) in m.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            let other = m.get(j).and_then(|r| r.get(i)).copied().unwrap_or(f64::NAN);
            assert!((v - other).abs() < 1e-15, "A[{i}][{j}] = {v}, A[{j}][{i}] = {other}");
        }
    }
}

#[test]
fn multiplying_by_the_operator_is_multiplying_by_that_matrix() {
    let occ = cube(4);
    let a = laplacian(&occ, 3.5);
    let m = dense(&a);
    let n = a.equations();
    let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.37).sin()).collect();
    let mut got = vec![0.0; n];
    a.mul(&x, &mut got).expect("multiply");
    for (e, row) in m.iter().enumerate() {
        let want: f64 = row.iter().zip(&x).map(|(c, v)| c * v).sum();
        let have = got.get(e).copied().unwrap_or(f64::NAN);
        assert!((have - want).abs() < 1e-12, "row {e}: {have} vs {want}");
    }
}

#[test]
fn pure_conduction_adds_up_to_nothing_in_every_row() {
    // Adding the same number to every temperature moves no heat, so a conduction operator with
    // no boundary condition on it has to annihilate a constant. A row that does not sum to zero
    // is an assembly that has invented a heat path.
    let occ = cube(5);
    let a = laplacian(&occ, 7.25);
    for (e, s) in a.row_sums().iter().enumerate() {
        assert!(s.abs() < 1e-12, "row {e} sums to {s}");
    }
    let n = a.equations();
    let ones = vec![1.0; n];
    let mut out = vec![f64::NAN; n];
    a.mul(&ones, &mut out).expect("multiply");
    assert!(out.iter().all(|v| v.abs() < 1e-12), "a constant is in the null space");
}

#[test]
fn conjugate_gradients_finds_the_answer_a_dense_solve_would() {
    let occ = cube(4);
    let mut a = laplacian(&occ, 2.0);
    // Anchor it: without something on the diagonal beyond the couplings there is no single
    // answer, which is the next test.
    for e in 0..a.equations() {
        a.add_diagonal(e, 0.75);
    }
    let n = a.equations();
    let want: Vec<f64> = (0..n).map(|i| 1.0 + (i as f64 * 0.11).cos()).collect();
    let mut b = vec![0.0; n];
    a.mul(&want, &mut b).expect("multiply");

    let mut got = vec![0.0; n];
    let report = a.solve(&b, &mut got, 1e-12, 1000).expect("solve");
    assert!(report.residual <= 1e-12, "{report:?}");
    for (e, (g, w)) in got.iter().zip(&want).enumerate() {
        assert!((g - w).abs() < 1e-8, "equation {e}: {g} vs {w}");
    }
    // And it is exact in at most as many steps as there are equations.
    assert!(report.iterations as usize <= n, "{report:?}");
}

#[test]
fn a_good_starting_guess_is_used_rather_than_thrown_away() {
    let occ = cube(4);
    let mut a = laplacian(&occ, 2.0);
    for e in 0..a.equations() {
        a.add_diagonal(e, 0.75);
    }
    let n = a.equations();
    let want: Vec<f64> = (0..n).map(|i| 1.0 + (i as f64 * 0.11).cos()).collect();
    let mut b = vec![0.0; n];
    a.mul(&want, &mut b).expect("multiply");
    let mut from_zero = vec![0.0; n];
    let cold = a.solve(&b, &mut from_zero, 1e-12, 1000).expect("solve");
    let mut from_answer = want.clone();
    let warm = a.solve(&b, &mut from_answer, 1e-12, 1000).expect("solve");
    assert_eq!(warm.iterations, 0, "starting at the answer is finishing");
    assert!(cold.iterations > 0, "{cold:?}");
}

#[test]
fn an_operator_with_nothing_holding_it_down_is_refused() {
    let occ = cube(4);
    let a = laplacian(&occ, 2.0);
    let n = a.equations();
    let b = vec![1.0; n];
    let mut x = vec![0.0; n];
    // Every corner cell has couplings, so the diagonal is positive -- but a cell in the middle
    // of a one-cell-thick slab would not be, and either way the operator is singular. The
    // diagonal check is what a caller meets first.
    match a.solve(&b, &mut x, 1e-10, 100) {
        Err(SimError::NoConvergence { .. }) | Err(SimError::NotDefinite(_)) => {}
        other => panic!("a singular operator was solved anyway: {other:?}"),
    }
}

#[test]
fn hostile_numbers_are_refused_rather_than_propagated() {
    let occ = cube(4);
    let mut a = laplacian(&occ, 2.0);
    for e in 0..a.equations() {
        a.add_diagonal(e, 1.0);
    }
    let n = a.equations();
    let mut x = vec![0.0; n];
    assert!(matches!(a.solve(&vec![1.0; n - 1], &mut x, 1e-10, 10), Err(SimError::Setup(_))));
    assert!(matches!(a.solve(&vec![1.0; n], &mut vec![0.0; n + 1], 1e-10, 10), Err(SimError::Setup(_))));
    assert!(matches!(a.solve(&vec![1.0; n], &mut x, 0.0, 10), Err(SimError::Setup(_))));
    assert!(matches!(a.solve(&vec![1.0; n], &mut x, 2.0, 10), Err(SimError::Setup(_))));
    let mut nan = vec![f64::NAN; n];
    assert!(matches!(a.solve(&vec![1.0; n], &mut nan, 1e-10, 10), Err(SimError::NotFinite(_))));
    let mut ok = vec![0.0; n];
    assert!(matches!(a.solve(&vec![f64::INFINITY; n], &mut ok, 1e-10, 10), Err(SimError::NotFinite(_))));
    let mut wrong = vec![0.0; n];
    assert!(a.mul(&vec![0.0; n + 2], &mut wrong).is_err());
}

#[test]
fn a_right_hand_side_of_nothing_has_the_answer_nothing() {
    let occ = cube(4);
    let mut a = laplacian(&occ, 2.0);
    for e in 0..a.equations() {
        a.add_diagonal(e, 1.0);
    }
    let n = a.equations();
    let mut x = vec![0.0; n];
    let report = a.solve(&vec![0.0; n], &mut x, 1e-10, 100).expect("solve");
    assert_eq!(report.iterations, 0);
    assert!(x.iter().all(|v| *v == 0.0));
}
