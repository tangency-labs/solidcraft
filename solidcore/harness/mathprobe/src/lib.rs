//! The std f64 functions solidcore calls, one export each (`p_` avoids clashing
//! with the libm symbols they are implemented by on wasm).

macro_rules! un { ($($n:ident => $f:ident),*) => { $(
    #[no_mangle] pub extern "C" fn $n(x: f64) -> f64 { x.$f() }
)* } }
un!(p_sin => sin, p_cos => cos, p_tan => tan, p_asin => asin, p_acos => acos,
    p_atan => atan, p_sinh => sinh, p_cosh => cosh, p_ln => ln, p_log10 => log10,
    p_cbrt => cbrt);

#[no_mangle]
pub extern "C" fn p_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
#[no_mangle]
pub extern "C" fn p_powf(x: f64, y: f64) -> f64 {
    x.powf(y)
}
#[no_mangle]
pub extern "C" fn p_powi(x: f64, n: f64) -> f64 {
    x.powi(n as i32)
}

/// (name, arity, function) for the native driver.
pub fn table() -> Vec<(&'static str, usize, fn(f64, f64) -> f64)> {
    vec![
        ("sin", 1, |x, _| p_sin(x)),
        ("cos", 1, |x, _| p_cos(x)),
        ("tan", 1, |x, _| p_tan(x)),
        ("asin", 1, |x, _| p_asin(x)),
        ("acos", 1, |x, _| p_acos(x)),
        ("atan", 1, |x, _| p_atan(x)),
        ("sinh", 1, |x, _| p_sinh(x)),
        ("cosh", 1, |x, _| p_cosh(x)),
        ("ln", 1, |x, _| p_ln(x)),
        ("log10", 1, |x, _| p_log10(x)),
        ("cbrt", 1, |x, _| p_cbrt(x)),
        ("atan2", 2, |y, x| p_atan2(y, x)),
        ("powf", 2, |x, y| p_powf(x, y)),
        ("powi", 2, |x, n| p_powi(x, n)),
    ]
}
