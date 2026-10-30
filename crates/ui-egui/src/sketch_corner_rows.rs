//! The corner rows of the sketch Fillet and Chamfer window: the pairs of picked lines that make
//! a corner, matched up as they are picked.
//!
//! Each row is a corner: two lines meeting at one of their ends, or a point picked on its own,
//! or a line still waiting for a partner. Only a corner of exactly two lines is cut, so the
//! window resolves which lines the user means instead of asking the command to guess at a point
//! where three or four lines meet.
//!
//! Picks are matched in the order they were made. A picked line looks for an earlier line with a
//! free end at one of its own ends: a line still waiting in a row of its own takes the new line
//! into its row, and a line already in a corner starts a second row at its other end. The shared
//! end becomes the corner and is used up, so a later line cannot pair there again. Two lines in
//! line with each other make no corner (a fillet and a chamfer both refuse them), so each keeps
//! a row of its own: selecting A and C with 180 degrees between them shows the two rows A and C.
//! Unpicking a line recomputes the rows from the picks that remain, which is how unpicking B
//! from the rows A-B, B-C leaves A and C.
//!
//! A place is what the ends are matched on, not a point: two profiles drawn apart and meeting at
//! one point (each with a corner point of its own, both held on the origin) share that corner.
//! Picking the two lines that meet there makes one row, and the commands cut that one corner.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec2;
use solidcraft_engine::sketch::{CurveKind, Sketch};

/// Points closer together than this are at one place, as the corner commands take them.
const TOGETHER: f64 = 1e-6;

/// The keys the rows keep their own state under, inside the dialog's extras.
const OFF: &str = "corner.off";
const FLIP: &str = "corner.flip";

/// What a click on a row changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Its X was clicked: the corner is cut, or no longer is, while the row stays.
    Off(bool),
    /// Its Flip was clicked: the two lines change places, so the first distance takes the other leg.
    Flip(bool),
}

/// A row of the window: a corner of two lines, a line waiting for a partner, or a point picked
/// on its own.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    /// The picked sides of the corner, in pick order. One while the row waits for a partner, or
    /// when only a point names the corner.
    pub lines: Vec<String>,
    /// The corner point the two lines share, or the point picked on its own.
    pub point: Option<String>,
    /// Stable name of the corner, for its own off and flip state.
    pub key: String,
    /// Whether OK still cuts this corner. Its X turns it off without taking the row away.
    pub enabled: bool,
    /// Whether the two lines are named in the other order.
    pub flip: bool,
}

impl Row {
    /// The row as the window names it: the sides picked for it.
    pub(crate) fn label(&self) -> String {
        match self.lines.as_slice() {
            [] => self.point.clone().unwrap_or_default(),
            lines => lines.join("-"),
        }
    }
}

/// The places of a sketch: the points at one position share one name, that of the first point
/// found there. Two profiles drawn apart have a corner point each where they meet, and their
/// corner is still one corner.
struct Sites {
    /// The point naming the place of each point index.
    name: Vec<String>,
}

impl Sites {
    fn new(sk: &Sketch) -> Self {
        // Points within `TOGETHER` are in the same or neighbouring cells, so one look at those
        // finds the place without comparing every pair of points.
        let cell = |p: Vec2| ((p.x / TOGETHER).floor() as i64, (p.y / TOGETHER).floor() as i64);
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        let mut name: Vec<String> = Vec::with_capacity(sk.points.len());
        for (index, point) in sk.points.iter().enumerate() {
            let (cx, cy) = cell(point.pos);
            let mut same = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let found = cells
                        .get(&(cx + dx, cy + dy))
                        .and_then(|list| list.iter().copied().find(|other| sk.points.get(*other).is_some_and(|p| p.pos.dist(point.pos) <= TOGETHER)));
                    same = same.or(found);
                }
            }
            match same.and_then(|other| name.get(other).cloned()) {
                Some(named) => name.push(named),
                None => {
                    cells.entry((cx, cy)).or_default().push(index);
                    name.push(point.id.clone());
                }
            }
        }
        Self { name }
    }

    /// The name of the place at `index`; empty for no such point.
    fn of(&self, index: usize) -> &str {
        self.name.get(index).map(String::as_str).unwrap_or_default()
    }

    /// The place a point reference such as `l1.end` or `origin` is at.
    fn named(&self, sk: &Sketch, id: &str) -> String {
        sk.resolve_point(id).map(|index| self.of(index).to_string()).unwrap_or_else(|| id.to_string())
    }
}

/// The corner rows the picks make, in the order they were found. `extra` holds the rows' own off
/// and flip state, which an unpick or a repick keeps as long as the corner stays the same.
pub(crate) fn rows(sk: &Sketch, picks: &[Sel], extra: &Map<String, Value>) -> Vec<Row> {
    let sites = Sites::new(sk);
    let mut found: Vec<Row> = Vec::new();
    // The ends a corner already uses: one per pair, and a point picked on its own uses one too.
    let mut used: Vec<String> = Vec::new();
    for pick in picks {
        match pick {
            Sel::SketchCurve { id, .. } => {
                let Some((a, b)) = ends(sk, id) else {
                    // A circle or a spline has no ends to meet another line at: a row of its own,
                    // which the command refuses with its own reason.
                    found.push(row(vec![id.clone()], None));
                    continue;
                };
                let mut paired = false;
                for point in [sites.of(a).to_string(), sites.of(b).to_string()] {
                    if used.contains(&point) {
                        continue;
                    }
                    // The first line picked before this one that ends there, and is not in line
                    // with it. A line already in a corner can still take a partner at its other end.
                    let partner = found.iter().enumerate().find_map(|(index, row)| {
                        if row.lines.iter().any(|line| line == id) {
                            return None;
                        }
                        row.lines
                            .iter()
                            .find(|line| ends(sk, line).is_some_and(|(x, y)| sites.of(x) == point || sites.of(y) == point))
                            .filter(|line| !in_line(sk, &sites, line, id, &point))
                            .map(|line| (index, line.clone()))
                    });
                    let Some((index, partner)) = partner else { continue };
                    if found[index].lines.len() == 1 && found[index].point.is_none() {
                        // The row waiting for its partner: the two lines now make its corner.
                        found[index].lines.push(id.clone());
                        found[index].point = Some(point.clone());
                        found[index].key = point.clone();
                    } else {
                        found.push(row(vec![partner, id.clone()], Some(point.clone())));
                    }
                    used.push(point.clone());
                    paired = true;
                }
                if !paired {
                    found.push(row(vec![id.clone()], None));
                }
            }
            Sel::SketchPoint { id } => {
                let point = sites.named(sk, id);
                if used.contains(&point) {
                    continue;
                }
                // A point joins the row of the line it ends, which is the corner there.
                let waiting = found.iter().position(|row| {
                    row.lines.len() == 1
                        && row.point.is_none()
                        && ends(sk, &row.lines[0]).is_some_and(|(a, b)| sites.of(a) == point || sites.of(b) == point)
                });
                match waiting {
                    Some(index) => {
                        found[index].point = Some(point.clone());
                        found[index].key = point.clone();
                    }
                    None => found.push(row(Vec::new(), Some(point.clone()))),
                }
                used.push(point);
            }
            _ => {}
        }
    }
    for row in &mut found {
        row.enabled = !listed(extra, OFF, &row.key);
        row.flip = listed(extra, FLIP, &row.key);
    }
    found
}

/// The corner as the commands take it: `a` and `b` the two lines (in the row's order, or the
/// other way round when it is flipped), or the point alone when the two lines are not both known.
pub(crate) fn params(sk: &Sketch, row: &Row) -> Value {
    match sides(sk, row, &Sites::new(sk)) {
        Some((a, b)) if row.flip => json!({"a": b, "b": a}),
        Some((a, b)) => json!({"a": a, "b": b}),
        None => row.point.as_ref().map(|point| json!({"point": point})).unwrap_or_else(|| json!({})),
    }
}

/// The row's two sides: the lines it holds, or the line it holds and the other one ending at its
/// point, or the two lines ending at a point picked on its own (in the order the command would
/// resolve them, so a flip names them the other way round).
fn sides(sk: &Sketch, row: &Row, sites: &Sites) -> Option<(String, String)> {
    match row.lines.as_slice() {
        [a, b] => Some((a.clone(), b.clone())),
        [a] => {
            let point = row.point.as_ref()?;
            let other = meeting(sk, point, sites).into_iter().find(|i| sk.curves.get(*i).is_some_and(|c| c.id != *a))?;
            Some((a.clone(), sk.curves.get(other)?.id.clone()))
        }
        [] => {
            let point = row.point.as_ref()?;
            match meeting(sk, point, sites)[..] {
                [a, b] => Some((sk.curves.get(a)?.id.clone(), sk.curves.get(b)?.id.clone())),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The lines ending at `point`, in curve order. More than two means no single corner: a
/// construction line at the point (a centre rectangle's diagonal) is not a side, as the command
/// itself decides.
fn meeting(sk: &Sketch, point: &str, sites: &Sites) -> Vec<usize> {
    let meets = |c: &solidcraft_engine::sketch::Curve| match c.kind.ends() {
        Some((p, q)) => sites.of(p) == point || sites.of(q) == point,
        None => false,
    };
    let mut lines: Vec<usize> = (0..sk.curves.len()).filter(|i| !sk.is_linked_curve(*i) && sk.curves.get(*i).is_some_and(meets)).collect();
    if lines.len() > 2 {
        lines.retain(|i| sk.curves.get(*i).is_some_and(|c| !c.construction));
    }
    lines
}

/// Turn a row's corner off or on, keeping the row.
pub(crate) fn set_off(extra: &mut Map<String, Value>, corner: &str, off: bool) {
    listed_set(extra, OFF, corner, off);
}

/// Flip a row's two legs, leaving every other row as it is.
pub(crate) fn set_flip(extra: &mut Map<String, Value>, corner: &str, flip: bool) {
    listed_set(extra, FLIP, corner, flip);
}

fn listed(extra: &Map<String, Value>, key: &str, corner: &str) -> bool {
    extra.get(key).and_then(Value::as_array).is_some_and(|list| list.iter().any(|value| value.as_str() == Some(corner)))
}

fn listed_set(extra: &mut Map<String, Value>, key: &str, corner: &str, on: bool) {
    let mut list: Vec<String> =
        extra.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    list.retain(|name| name != corner);
    if on {
        list.push(corner.to_string());
    }
    extra.insert(key.to_string(), json!(list));
}

fn row(lines: Vec<String>, point: Option<String>) -> Row {
    let key = point.clone().or_else(|| lines.first().cloned()).unwrap_or_default();
    Row { lines, point, key, enabled: true, flip: false }
}

/// The point indices at the ends of a curve that has two of them.
fn ends(sk: &Sketch, id: &str) -> Option<(usize, usize)> {
    match sk.curves.get(sk.curve_index(id)?)?.kind {
        CurveKind::Line { a, b } | CurveKind::Arc { a, b, .. } => Some((a, b)),
        _ => None,
    }
}

/// Whether two lines run in line with each other at `point`: 0 or 180 degrees between them, which
/// makes no corner to cut. Arcs are left to the command, which knows their tangents.
fn in_line(sk: &Sketch, sites: &Sites, a: &str, b: &str, point: &str) -> bool {
    let dir = |id: &str| -> Option<Vec2> {
        let CurveKind::Line { a, b } = sk.curves.get(sk.curve_index(id)?)?.kind else { return None };
        let (pa, pb) = (sk.points.get(a)?, sk.points.get(b)?);
        if sites.of(a) == point {
            Some((pb.pos - pa.pos).normalized()?)
        } else if sites.of(b) == point {
            Some((pa.pos - pb.pos).normalized()?)
        } else {
            None
        }
    };
    match (dir(a), dir(b)) {
        (Some(u), Some(v)) => u.cross(v).abs() <= 1e-9 && u.dot(v) != 0.0,
        _ => false,
    }
}
