//! Replay frozen IsoCurve continuity inputs through the public native API.
use serde::{Deserialize, Serialize};
use solidcore::{
    curve::{curve::Curve, iso_curve::IsoCurve},
    nurbs::{
        b_spline_surface::{BSplineSurface, SurfParamType},
        math::CPoint,
        srf_basic::Surface as Net,
    },
    Kernel,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// Curve parameters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Params0 {
    sp: i32,
    iso: f64,
    #[serde(default)]
    from_left: bool,
    #[serde(default = "yes")]
    clamp: bool,
}

/// Preserve the fixture default that clamps the fixed isoparameter to its domain.
fn yes() -> bool {
    true
}

/// A curve: surface (`None` = no NURB) and parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveIn {
    srf: Option<SrfIn>,
    #[serde(flatten)]
    params: Params0,
}

/// The other curve of `eq`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OtherIn {
    Own(CurveIn),
    Itself,
    SameSurface(Params0),
}

/// The iso data and surface written into a database stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbIn {
    sp: u64,
    iso: f64,
    from_left: i32,
    srf_type: i64,
    srf: SrfIn,
    #[serde(default = "one")]
    kt: u64,
    #[serde(default = "nine")]
    form: u64,
}

/// Preserve the database fixture's default knot-type code.
fn one() -> u64 {
    1
}

/// Preserve the database fixture's default surface-form code.
fn nine() -> u64 {
    9
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State,
    Knots {
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Evaluate {
        t: f64,
        n: usize,
        from_left: bool,
        non_zero: bool,
    },
    EvaluatePoint {
        t: f64,
    },
    Continuities {
        tol: f64,
    },
    Eq {
        other: OtherIn,
    },
    IsKindOf {
        t: i64,
    },
    TypeStrings,
    UvIsoLine,
    ReadDb {
        binary: bool,
        version: u64,
        db: DbIn,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CurveIn>,
    #[serde(flatten)]
    op: OpIn,
}

/// Replay inline continuity records from the supplied JSONL file as native results.
fn main() {
    for line in std::fs::read_to_string(std::env::args().nth(1).unwrap())
        .unwrap()
        .lines()
    {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        let name = record["case"].as_str().unwrap();
        let i: Inputs = serde_json::from_value(record["input"].clone()).unwrap();
        if let OpIn::Continuities { tol } = i.op {
            let c = i.curve.as_ref().unwrap();
            if let Some(SrfIn::Inline(s)) = &c.srf {
                let k = Kernel::default();
                let rows = s
                    .net
                    .iter()
                    .map(|r| {
                        r.iter()
                            .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
                            .collect()
                    })
                    .collect();
                let net = Net::new(rows, s.p, s.q, s.u.clone(), s.v.clone()).unwrap();
                let bs = BSplineSurface::from_nurb(net, false);
                let sp = match c.params.sp {
                    0 => SurfParamType::U,
                    1 => SurfParamType::V,
                    2 => SurfParamType::Unknown,
                    3 => SurfParamType::Both,
                    4 => SurfParamType::Neither,
                    5 => SurfParamType::UMin,
                    6 => SurfParamType::VMin,
                    7 => SurfParamType::UMax,
                    8 => SurfParamType::VMax,
                    _ => SurfParamType::Out,
                };
                let cv = Curve::from(IsoCurve::new(
                    bs,
                    sp,
                    c.params.iso,
                    c.params.from_left,
                    c.params.clamp,
                ));
                let result = cv.calculate_continuities(&k, tol);
                let out = result.map(|(m, v)| {
                    let mut out = vec![0., m as u8 as f64, v.len() as f64];
                    out.extend(v.iter().map(|x| *x as u8 as f64));
                    out
                });
                println!(
                    "{}",
                    serde_json::json!({"case":name,"input":i,"out":out.ok()})
                );
            }
        }
    }
}
