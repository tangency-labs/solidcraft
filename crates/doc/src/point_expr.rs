//! The points a sketch places by expressions: the points whose expressions read another
//! point's coordinate (`p1.x`), and the order those points can be worked out in (#19232).

use std::collections::BTreeMap;

use solidcraft_sketch::Sketch;

use crate::expr::{self, Kind, Value};
use crate::{DocError, Result};

impl crate::Document {
    /// The value of a point's coordinate expression: the design's parameters, and the
    /// coordinates of the sketch's points (`p1.x`). A point that is placed by an
    /// expression is read where it is now, as the sketch is being drawn.
    pub fn eval_point_expr(&self, sk: &Sketch, e: &str) -> Result<f64> {
        // The parameters are evaluated only when the expression names one: most expressions an
        // agent sends are plain values.
        let vals: std::cell::OnceCell<BTreeMap<String, Value>> = std::cell::OnceCell::new();
        let lookup =
            |n: &str| vals.get_or_init(|| self.param_values().0).get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        let members = |point: &str, member: &str| coordinate(sk, point, member);
        expr::eval_with_members(e, &lookup, &members)?.to_kind(Kind::Length)
    }
}

/// The points whose coordinates the expressions of the point at `index` read.
pub(crate) fn reads(sk: &Sketch, index: usize) -> Vec<String> {
    let Some(exprs) = sk.points.get(index).and_then(|p| p.exprs.as_deref()) else { return Vec::new() };
    [exprs.x.as_deref(), exprs.y.as_deref()].into_iter().flatten().flat_map(expr::point_members).map(|(point, _)| point).collect()
}

/// The coordinate of a point of the sketch: where it is now. A point that is not placed by an
/// expression is where it was: the solve may move it later, and a point that reads it does
/// not follow that move.
pub(crate) fn coordinate(sk: &Sketch, point: &str, member: &str) -> Result<Value> {
    let pos = sk.resolve_point(point).and_then(|i| sk.points.get(i)).map(|p| p.pos);
    let Some(pos) = pos else {
        return Err(DocError::Expr(format!("unknown sketch point `{point}`")));
    };
    match member {
        "x" => Ok(Value::length(pos.x)),
        "y" => Ok(Value::length(pos.y)),
        other => Err(DocError::Expr(format!("a point's coordinate is `x` or `y`, not `{other}`"))),
    }
}

/// The order the points placed by expressions can be worked out in: a point waits for the
/// points its expressions read. A cycle among them is refused, naming the points in it.
pub(crate) fn order(placed: &[usize], names: &[Vec<String>], sk: &Sketch) -> Result<Vec<usize>> {
    // The point each name is, among the ones placed by expressions. A name of any other point
    // is read where that point is, so it does not wait for anything.
    let index = |name: &str| {
        let point = sk.resolve_point(name)?;
        placed.iter().position(|&i| i == point)
    };
    let mut order: Vec<usize> = Vec::with_capacity(placed.len());
    // 0: not reached yet, 1: being worked out, 2: worked out.
    let mut state: Vec<u8> = vec![0; placed.len()];
    // The points being worked out, and the name each of them reads next.
    let mut path: Vec<usize> = Vec::new();
    let mut next: Vec<usize> = Vec::new();
    for start in 0..placed.len() {
        if state[start] != 0 {
            continue;
        }
        state[start] = 1;
        path.push(start);
        next.push(0);
        while let Some(&k) = path.last() {
            let at = path.len() - 1;
            if next[at] < names.get(k).map_or(0, Vec::len) {
                let name = names[k][next[at]].clone();
                next[at] += 1;
                let Some(j) = index(&name) else { continue };
                match state[j] {
                    0 => {
                        state[j] = 1;
                        path.push(j);
                        next.push(0);
                    }
                    1 => {
                        // A cycle: the points in it, from where it starts.
                        let from = path.iter().position(|&s| s == j).unwrap_or(0);
                        let mut ids: Vec<String> = path[from..].iter().map(|&s| id(sk, placed[s])).collect();
                        ids.push(id(sk, placed[j]));
                        return Err(DocError::Expr(format!("circular reference: {}", ids.join(" → "))));
                    }
                    _ => {}
                }
            } else {
                state[k] = 2;
                order.push(placed[k]);
                path.pop();
                next.pop();
            }
        }
    }
    Ok(order)
}

/// The id of the point at `index`.
fn id(sk: &Sketch, index: usize) -> String {
    sk.points.get(index).map(|p| p.id.clone()).unwrap_or_default()
}
