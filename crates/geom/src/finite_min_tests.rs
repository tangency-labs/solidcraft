use super::*;

#[test]
fn empty_has_no_best() {
    assert_eq!(std::iter::empty::<f64>().min_by_finite(|x| *x), None);
    assert_eq!(std::iter::empty::<f64>().max_by_finite(|x| *x), None);
}

#[test]
fn non_finite_keys_never_win() {
    let v = [f64::NAN, 3.0, f64::NEG_INFINITY, 1.0, f64::NAN, f64::INFINITY, 2.0];
    assert_eq!(v.iter().min_by_finite(|x| **x), Some(&1.0));
    assert_eq!(v.iter().max_by_finite(|x| **x), Some(&3.0));
    assert_eq!([f64::NAN, f64::INFINITY].iter().min_by_finite(|x| **x), None);
    assert_eq!([f64::NAN, f64::INFINITY].iter().max_by_finite(|x| **x), None);
}

#[test]
fn ties_keep_the_first() {
    let v = [(2.0, 'a'), (1.0, 'b'), (5.0, 'c'), (1.0, 'd'), (5.0, 'e')];
    assert_eq!(v.iter().min_by_finite(|x| x.0), Some(&(1.0, 'b')));
    assert_eq!(v.iter().max_by_finite(|x| x.0), Some(&(5.0, 'c')));
}
