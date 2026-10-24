//! Lossless native ellipse/circle records used by the B-rep graph importers.

use solidcore::curve::curve::p2::{CIRCLE_TYPE, ELLIPSE_TYPE};
use solidcore::curve::curve::{Curve as SmCurve, CurveKind};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};

use crate::HarnessError;

fn harness(message: impl Into<String>) -> HarnessError {
    HarnessError::new(message)
}

/// Reads the head after its `-3` marker; the independent B-spline block follows it.
/// Counts, bytes, concrete kind and complete consumption are checked before success.
pub(crate) fn read_head(
    num: &mut dyn FnMut() -> Result<f64, HarnessError>,
    remaining: usize,
) -> Result<SmCurve, HarnessError> {
    let integer = |num: &mut dyn FnMut() -> Result<f64, HarnessError>,
                   max: f64|
     -> Result<u64, HarnessError> {
        let n = num()?;
        if !n.is_finite() || !(0.0..=max).contains(&n) || n.fract() != 0.0 {
            return Err(harness("export: bad exact curve head"));
        }
        Ok(n as u64)
    };
    let ty = integer(num, 1e6)? as i64;
    if ty != ELLIPSE_TYPE && ty != CIRCLE_TYPE {
        return Err(harness("export: unsupported exact curve type"));
    }
    let version = integer(num, 1e4)?;
    let dim = integer(num, 3.0)?;
    if dim != 3 {
        return Err(harness("export: exact edge curve is not 3D"));
    }
    let oob = integer(num, 1.0)? != 0;
    let n = integer(num, 1e8)? as usize;
    if n > remaining.saturating_sub(5) {
        return Err(harness("export too short for exact curve record"));
    }
    let mut bytes = Vec::new();
    for _ in 0..n {
        bytes.push(integer(num, 255.0)? as u8);
    }
    let mut db = DatabaseReader::with_stream(FileType::Binary, InputStream::new(bytes));
    db.set_version(version);
    let mut exact = SmCurve::read_from_db(&Kernel::default(), ty, &mut db, dim, version)
        .map_err(|e| harness(format!("export: exact curve record: {e}")))?;
    if db.in_stream().is_none_or(|s| s.remaining() != 0) {
        return Err(harness("export: exact curve record not read whole"));
    }
    let CurveKind::Ellipse(ellipse) = exact.kind_mut() else {
        return Err(harness("export: exact curve reader changed its kind"));
    };
    if ellipse.type_code() != ty {
        return Err(harness(
            "export: exact curve reader changed circle identity",
        ));
    }
    ellipse.set_out_of_bounds_enabled(oob);
    Ok(exact)
}

/// Checks the native record against the separately exported stored NURB.
pub(crate) fn check_base(exact: &SmCurve, fallback: Option<&SmCurve>) -> Result<(), HarnessError> {
    let CurveKind::Ellipse(ellipse) = exact.kind() else {
        return Err(harness("export: exact curve reader changed its kind"));
    };
    let Some(CurveKind::BSpline(base)) = fallback.map(SmCurve::kind) else {
        return Err(harness("export: exact curve without its B-spline part"));
    };
    if curve_base_bits(ellipse.base()) != curve_base_bits(base) {
        return Err(harness("export: exact curve B-spline part differs"));
    }
    Ok(())
}

fn curve_base_bits(base: &BSplineCurve) -> Vec<u64> {
    let mut bits = vec![base.dim()];
    match base.gw_nurb() {
        None => bits.push(0),
        Some(n) => {
            bits.extend([1, n.n() as u64, n.degree() as u64, n.m() as u64]);
            for p in n.cpts() {
                bits.extend([p.x, p.y, p.z, p.w].map(f64::to_bits));
            }
            bits.extend(n.knots().iter().map(|v| v.to_bits()));
        }
    }
    bits
}
