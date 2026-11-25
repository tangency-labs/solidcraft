//! Shared scan of the #2479 probes (`#[path]`-included): every UV trim curve of a
//! body whose end parameters, or the UV points it evaluates there, fall outside its
//! face surface's parameter domain, and by how many ulps.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    dead_code
)] // test helpers; each probe uses a subset

use solidcore::geom::extent1d::Extent1d;
use solidcore::topology::edgeuse::p1::uv_trim_curve_pointer;
use solidcore::topology::face::p2::{loops, surface};
use solidcore::topology::loopuse::loop_edgeuses;
use solidcore::{Brep, FaceId, Kernel};

/// One end of a UV trim curve outside its face's domain.
#[derive(Debug, Clone, PartialEq)]
pub struct OutOfDomain {
    /// The face whose loop holds the edge use.
    pub face: FaceId,
    /// The face surface's kind (`GetTypeString`).
    pub surface: &'static str,
    /// The edge use's index.
    pub edgeuse: u32,
    /// `"t0"`/`"t1"` for the curve's interval ends, `"u(t0)"`... for the coordinates
    /// of the points it evaluates there.
    pub what: &'static str,
    /// The value outside the domain.
    pub value: f64,
    /// The bound it passes.
    pub bound: f64,
    /// How far past the bound, in ulps of the bound.
    pub ulps: u64,
}

/// The number of doubles from `a` up to `b` (`a <= b`, both finite).
pub fn ulps_between(a: f64, b: f64) -> u64 {
    fn key(x: f64) -> i64 {
        let i = x.to_bits() as i64;
        if i < 0 {
            i64::MIN - i
        } else {
            i
        }
    }
    (key(b) - key(a)).unsigned_abs()
}

fn check(
    out: &mut Vec<OutOfDomain>,
    proto: &OutOfDomain,
    what: &'static str,
    v: f64,
    ivl: &Extent1d,
) {
    let (lo, hi) = (ivl.min(), ivl.max());
    let past = if v < lo {
        Some((lo, ulps_between(v, lo)))
    } else if v > hi {
        Some((hi, ulps_between(hi, v)))
    } else {
        None
    };
    if let Some((bound, ulps)) = past {
        out.push(OutOfDomain {
            what,
            value: v,
            bound,
            ulps,
            ..proto.clone()
        });
    }
}

/// Every UV trim curve end of `b`'s faces outside the face surface's natural UV domain
/// (`GetNaturalUVDomain`, with a lazy NURB made), in face, loop and edge use order.
pub fn scan(k: &Kernel, b: &Brep) -> Vec<OutOfDomain> {
    let mut out = Vec::new();
    for (f, _) in b.faces().iter() {
        let Some(srf) = surface(b, f).unwrap() else {
            continue;
        };
        let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
        let (du, dv) = (dom.u_interval(), dom.v_interval());
        for l in loops(b, f, false, None).unwrap() {
            for eu in loop_edgeuses(b, l).unwrap() {
                let Some(c) = uv_trim_curve_pointer(b, eu).unwrap() else {
                    continue;
                };
                let proto = OutOfDomain {
                    face: f,
                    surface: srf.type_string(),
                    edgeuse: eu.index(),
                    what: "",
                    value: 0.0,
                    bound: 0.0,
                    ulps: 0,
                };
                let ivl = c.natural_interval();
                let p0 = c.evaluate_point(k, ivl.min()).unwrap();
                let p1 = c.evaluate_point(k, ivl.max()).unwrap();
                check(&mut out, &proto, "u(t0)", p0.x, &du);
                check(&mut out, &proto, "v(t0)", p0.y, &dv);
                check(&mut out, &proto, "u(t1)", p1.x, &du);
                check(&mut out, &proto, "v(t1)", p1.y, &dv);
            }
        }
    }
    out
}

/// [`scan`] as one line per finding, for assertion messages and probe output.
pub fn report(k: &Kernel, b: &Brep) -> Vec<String> {
    scan(k, b)
        .into_iter()
        .map(|o| {
            format!(
                "face {:?} ({}) eu {} {} = {:e} past {:e} by {} ulps",
                o.face, o.surface, o.edgeuse, o.what, o.value, o.bound, o.ulps
            )
        })
        .collect()
}
