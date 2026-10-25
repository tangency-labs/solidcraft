//! Runner-local native B-rep record decoding, shared with non-C++ regression tests.
//!
//! Deviations: validate counts before conversion, bound declared records and lists
//! by available tokens before iteration, and grow storage incrementally. Valid
//! record order, integer conversion and floating-point bits remain unchanged.

use crate::HarnessError;

/// A plain integer, record link, ordered link list, or real field.
#[derive(Clone, Copy)]
enum Slot {
    I,
    R(usize),
    L(usize),
    F,
}
use Slot::{F, I, L, R};

/// Fields in the shim's `emit_records` order, one layout per record kind.
const LAYOUT: [&[Slot]; 10] = [
    // region: in B-rep, shells
    &[I, L(1)],
    // shell: region, type, list head present, face/edge/vertex use heads
    &[R(0), I, I, R(2), R(6), R(8)],
    // face use: shell, face, mate, orientation, loop uses
    &[R(1), R(3), R(2), I, L(4)],
    // face: face use, domain, trim, tolerance, surface flags, natural domain,
    // evaluated flag and centre point
    &[R(2), F, F, F, F, I, F, I, I, F, F, F, F, I, F, F, F],
    // loop use: face use, loop, mate, start type, edge/vertex use, orientation
    &[R(2), R(5), R(4), I, R(6), R(8), I],
    // loop: loop use
    &[R(4)],
    // edge use: edge, owner type, loop use, shell, cw, ccw, vertex use,
    // orientation, UV trim present
    &[R(7), I, R(4), R(1), R(6), R(6), R(8), I, I],
    // edge: tolerance, interval, edge uses, curve present, natural interval
    &[F, F, F, L(6), I, F, F],
    // vertex use: vertex, owner type, edge use, loop use, shell, UV point
    &[R(9), I, R(6), R(4), R(1), F, F],
    // vertex: point, tolerance, vertex uses
    &[F, F, F, F, L(8)],
];

/// One record, retaining labels, ordered integer links and real values.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Rec {
    pub(super) label: i64,
    pub(super) ints: Vec<i64>,
    pub(super) links: Vec<Option<usize>>,
    pub(super) floats: Vec<f64>,
}

impl Rec {
    pub(super) fn new(label: i64) -> Self {
        Self {
            label,
            ints: Vec::new(),
            links: Vec::new(),
            floats: Vec::new(),
        }
    }
    pub(super) fn i(&mut self, x: i64) {
        self.ints.push(x);
        self.links.push(None);
    }
    pub(super) fn r(&mut self, kind: usize, x: i64) {
        self.ints.push(x);
        self.links.push(Some(kind));
    }
    pub(super) fn l(&mut self, kind: usize, xs: &[i64]) {
        self.i(xs.len() as i64);
        for &x in xs {
            self.r(kind, x);
        }
    }
    pub(super) fn f(&mut self, x: f64) {
        self.floats.push(x);
    }
}

/// Records in the shim's ten-kind walk order.
pub(super) type Model = [Vec<Rec>; 10];

fn count(value: f64, name: &str) -> Result<usize, HarnessError> {
    // usize::MAX rounds up on 64-bit hosts; use the exclusive power-of-two bound.
    let upper = 2.0_f64.powi(usize::BITS as i32);
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= upper {
        return Err(HarnessError::new(format!("bad {name} count {value}")));
    }
    Ok(value as usize)
}

fn next(input: &mut std::slice::Iter<'_, f64>) -> Result<f64, HarnessError> {
    input
        .next()
        .copied()
        .ok_or_else(|| HarnessError::new("short record output"))
}

/// Decodes the records the shim wrote, rejecting malformed counts and tails.
pub(super) fn parse_records(v: &[f64]) -> Result<Model, HarnessError> {
    let mut input = v.iter();
    let mut model: Model = Default::default();
    for (slots, out) in LAYOUT.iter().zip(model.iter_mut()) {
        let n = count(next(&mut input)?, "record")?;
        // Each record needs its label and at least one token per layout slot
        // (including the count of any empty list). Division avoids overflow.
        if n > input.len() / (slots.len() + 1) {
            return Err(HarnessError::new("short record output: record count"));
        }
        for _ in 0..n {
            let mut rec = Rec::new(next(&mut input)? as i64);
            for slot in *slots {
                match *slot {
                    I => rec.i(next(&mut input)? as i64),
                    R(k) => rec.r(k, next(&mut input)? as i64),
                    L(k) => {
                        let count = count(next(&mut input)?, "list")?;
                        if count > input.len() {
                            return Err(HarnessError::new("short record output: list count"));
                        }
                        rec.i(count as i64);
                        for _ in 0..count {
                            rec.r(k, next(&mut input)? as i64);
                        }
                    }
                    F => rec.f(next(&mut input)?),
                }
            }
            out.push(rec);
        }
    }
    if !input.as_slice().is_empty() {
        return Err(HarnessError::new("trailing record output"));
    }
    Ok(model)
}
